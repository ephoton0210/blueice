// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

impl Vm {
    /// `ToIntegerWithTruncation`, plus a range check: every Temporal numeric
    /// date/time field (year, month, day, era year, hour, ...) uses this
    /// same conversion in every context — constructor argument or
    /// property-bag field — truncating a fractional value toward zero
    /// rather than rejecting it (Test262's `PlainDate/argument-convert.js`,
    /// `PlainDate/prototype/with/order-of-operations.js`'s `year: 1.7`).
    pub(in super::super) fn temporal_integer(
        &mut self,
        value: &Value,
        minimum: i32,
        maximum: i32,
        name: &str,
    ) -> Result<i32, RuntimeError> {
        let value = self.coerce_number(value)?;
        let value = value.trunc();
        if !value.is_finite() || !(f64::from(minimum)..=f64::from(maximum)).contains(&value) {
            return Err(RuntimeError::RangeError(format!("invalid Temporal {name}")));
        }
        Ok(value as i32)
    }

    /// `ToBigInt`: a Number is a `TypeError` (not a truncation), a Boolean is
    /// `0n`/`1n`, and a String that is not an integer literal is a
    /// `SyntaxError` — the exact set Test262's `Instant/basic.js` and
    /// `Instant/argument.js` pin for the constructor's argument.
    pub(in super::super) fn temporal_to_big_int(
        &mut self,
        value: &Value,
    ) -> Result<BigInt, RuntimeError> {
        match self.coerce_primitive(value, "number")? {
            Value::BigInt(value) => Ok(value),
            Value::Bool(flag) => Ok(BigInt::from(u8::from(flag))),
            Value::String(text) => {
                let text = text
                    .to_utf8()
                    .map_err(|_| RuntimeError::SyntaxError("invalid BigInt string".into()))?;
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return Ok(BigInt::from(0));
                }
                BigInt::parse_bytes(trimmed.as_bytes(), 10)
                    .ok_or_else(|| RuntimeError::SyntaxError("invalid BigInt string".into()))
            }
            _ => Err(RuntimeError::TypeError(
                "Temporal.Instant requires epoch nanoseconds as a BigInt".into(),
            )),
        }
    }

    /// The raw `Temporal.PlainDate`/`PlainDateTime`/etc. **constructor**'s
    /// own positional `calendar` argument: a bare calendar ID string only.
    /// Test262's `calendar-invalid-iso-string.js` confirms a full
    /// date-with-annotation string (`"1997-12-04[u-ca=iso8601]"`) is a
    /// `RangeError` here specifically, unlike [`Self::temporal_calendar`]'s
    /// wider grammar below.
    pub(in super::super) fn temporal_calendar(
        &mut self,
        value: &Value,
    ) -> Result<String, RuntimeError> {
        if *value == Value::Undefined {
            return Ok("iso8601".into());
        }
        // Spec step: "If calendar is not a String, throw a TypeError
        // exception" -- no `ToString` coercion at all, unlike most other
        // Temporal string arguments. Pinned by
        // `calendar-wrong-type.js` (identical fixture across
        // `PlainDate`/`PlainDateTime`/`PlainMonthDay`/`PlainYearMonth`/
        // `ZonedDateTime`'s numeric constructors, all sharing this
        // function): `null`/`Boolean`/`Number`/`BigInt`/`Symbol`/a plain
        // object/a `Temporal.Duration` instance must all throw `TypeError`
        // immediately rather than being stringified first.
        let Value::String(value) = value else {
            return Err(RuntimeError::TypeError(
                "Temporal calendar must be a string".into(),
            ));
        };
        let value = value
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar".into()))?;
        canonical_calendar_id(&value)
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal calendar".into()))
    }

    /// `ToTemporalCalendarIdentifier`, used everywhere *other* than the raw
    /// constructor's own positional `calendar` argument: a property-bag
    /// `calendar` field (`Temporal.PlainDate.from({..., calendar})`) or a
    /// `Temporal.PlainDate.prototype.withCalendar` argument may themselves
    /// be a full date/date-time/offset/time/year-month/month-day string,
    /// not only a bare calendar ID — Test262's `since/calendar-id-match.js`
    /// passes `"2024-05-16[u-ca=iso8601]"` as a property-bag `calendar`
    /// value, `withCalendar/calendar-time-string.js` passes
    /// `"T11:30[u-ca=hebrew]"` to `withCalendar`, and an *unannotated* ISO
    /// string like `"2020-01-01"` or `"2020-01"` is equally valid and
    /// always means `"iso8601"`. A bare calendar ID wins outright (step 3,
    /// e.g. `"gregory"`); otherwise the whole string must parse as *some*
    /// recognized Temporal string shape (step 4 — date-time, year-month,
    /// month-day or time, matching `TemporalCalendarString`'s own grammar
    /// alternation), whose calendar annotation (if any) is the result — an
    /// unannotated but otherwise syntactically valid string implies
    /// `"iso8601"`, it is not itself tried as a calendar-ID literal. Before
    /// this fix, a bracket-less date-like string always fell straight to
    /// `canonical_calendar_id(&value)` and always failed, since no real
    /// calendar ID looks like a date — pinned by
    /// `PlainYearMonth/PlainMonthDay/prototype/equals/
    /// argument-propertybag-calendar-iso-string.js`'s eight unannotated/
    /// annotated date, dateTime, year-month and month-day forms.
    ///
    /// A Temporal object with its own `[[Calendar]]` slot returns that
    /// calendar directly (`ToTemporalCalendar` step 1.a's fast path,
    /// pinned by `TemporalHelpers.checkToTemporalCalendarFastPath`, which
    /// makes both a `calendar`/`calendarId` JS-visible property throw if
    /// read); only `PlainDate`/`PlainDateTime`/`PlainMonthDay`/
    /// `PlainYearMonth`/`ZonedDateTime` carry `[[InitializedTemporalXxx]]`
    /// calendar semantics — `Temporal.Duration`/`Instant`/`PlainTime` are
    /// calendar-less and fall through to the plain-string handling below,
    /// where a non-`String` value is a `TypeError`, not coerced via
    /// `ToString` the way a plain calendar-agnostic string argument would
    /// be (`argument-propertybag-calendar-wrong-type.js`'s ten cases:
    /// `null`/`Boolean`/`Number`/`BigInt`/`Symbol`/a plain object/a
    /// non-fast-path `Temporal.Duration` instance — this function is also
    /// exactly what a `Duration.prototype.round`'s `relativeTo` property
    /// bag's own `calendar` field goes through, per
    /// `relativeto-propertybag-calendar-wrong-type.js`).
    pub(in super::super) fn temporal_calendar_identifier(
        &mut self,
        value: &Value,
    ) -> Result<String, RuntimeError> {
        if *value == Value::Undefined {
            return Ok("iso8601".into());
        }
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if matches!(
                    temporal.kind,
                    TemporalKind::PlainDate
                        | TemporalKind::PlainDateTime
                        | TemporalKind::PlainMonthDay
                        | TemporalKind::PlainYearMonth
                        | TemporalKind::ZonedDateTime
                ) {
                    return Ok(temporal.calendar);
                }
            }
        }
        let Value::String(value) = value else {
            return Err(RuntimeError::TypeError(
                "Temporal calendar must be a string".into(),
            ));
        };
        let value = value
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar".into()))?;
        if let Some(id) = canonical_calendar_id(&value) {
            return Ok(id);
        }
        let parsed = iso::parse_date_time(&value)
            .or_else(|| iso::parse_year_month(&value))
            .or_else(|| iso::parse_month_day(&value))
            .or_else(|| iso::parse_time(&value));
        if let Some(parsed) = parsed {
            let calendar = parsed.calendar.as_deref().unwrap_or("iso8601");
            return canonical_calendar_id(calendar)
                .ok_or_else(|| RuntimeError::RangeError("invalid Temporal calendar".into()));
        }
        Err(RuntimeError::RangeError("invalid Temporal calendar".into()))
    }

    pub(in super::super) fn temporal_calendar_fields(
        &self,
        value: &TemporalValue,
    ) -> Result<TemporalCalendarFields, RuntimeError> {
        // The `"iso8601"` calendar is a fast path that deliberately never
        // reaches `icu_calendar::Date::try_new_iso` below: that constructor
        // enforces `icu_calendar`'s own `CONSTRUCTOR_YEAR_RANGE` (-9999..=9999
        // in the pinned `icu_calendar`), which is far narrower than
        // Temporal's own representable range (roughly ±271,821 years,
        // enforced separately by `epoch::is_date_within_limits` at
        // construction time). Without this fast path, `.year`/`.month`/
        // `.day`/etc. getters on an in-range extreme-year ISO date -- one
        // that *constructed* successfully -- would throw a spurious
        // `RangeError` from this getter dispatch alone. ISO fields are
        // exactly the value's own stored ISO date by definition (no
        // conversion needed), and the ISO calendar has no eras and always
        // twelve months, so this bypasses `icu_calendar` entirely rather
        // than special-casing its error path.
        // See development/browser_core/phase-26-ecma262-temporal/PLAN.md.
        if value.calendar == "iso8601" {
            return Ok(TemporalCalendarFields {
                year: value.year,
                month: value.month,
                month_code: format!("M{:02}", value.month),
                day: value.day,
                era: None,
                era_year: None,
                months_in_year: 12,
                days_in_month: plain_date::iso_days_in_month(value.year, value.month),
                days_in_year: if plain_date::is_iso_leap_year(value.year) {
                    366
                } else {
                    365
                },
                in_leap_year: plain_date::is_iso_leap_year(value.year),
            });
        }
        let calendar = calendar::calendar_kind(&value.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let iso = Date::try_new_iso(value.year, value.month, value.day)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal ISO date".into()))?;
        let date = iso.to_calendar(AnyCalendar::new(calendar));
        let year = date.year();
        let month = date.month();
        // The `iso8601` calendar never reaches this point (see the fast
        // path above), so every calendar here keeps ICU4X's own era, if any.
        let era = year.era();
        Ok(TemporalCalendarFields {
            year: year.extended_year(),
            // Temporal's numeric `month` is the ordinal month in a year.
            // A leap month therefore increments every following ordinal,
            // whereas a MonthCode retains the calendar's base month plus L.
            month: month.ordinal,
            month_code: month.to_input().code().to_string(),
            day: date.day_of_month().0,
            era: era.map(|era| era.era.to_string()),
            era_year: era.map(|era| era.year),
            months_in_year: date.months_in_year(),
            days_in_month: date.days_in_month(),
            days_in_year: date.days_in_year(),
            in_leap_year: date.is_in_leap_year(),
        })
    }

    pub(in super::super) fn temporal_value_from_calendar_date(
        kind: TemporalKind,
        calendar: String,
        date: Date<AnyCalendar>,
    ) -> TemporalValue {
        let iso = date.to_calendar(Iso);
        TemporalValue {
            kind,
            duration: None,
            year: iso.year().extended_year(),
            month: iso.month().number(),
            day: iso.day_of_month().0,
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

    pub(in super::super) fn temporal_plain_date_from_fields(
        &mut self,
        kind: TemporalKind,
        bag: &Value,
        overflow: OverflowInput,
    ) -> Result<TemporalValue, RuntimeError> {
        let calendar_value = self.get_property(bag, &"calendar".into())?;
        let calendar = self.temporal_calendar_identifier(&calendar_value)?;
        let is_date_time = kind == TemporalKind::PlainDateTime;

        // `PrepareCalendarFields` reads and immediately coerces every
        // recognized property in strict alphabetical order -- `day`,
        // `era`, `eraYear`, `hour`, `microsecond`, `millisecond`, `minute`,
        // `month`, `monthCode`, `nanosecond`, `second`, `year` --
        // interleaved with each field's own immediate conversion, never
        // batched into a "read everything, then convert everything" pass.
        // The six time-of-day fields only apply when `kind` is
        // `PlainDateTime`, including when this function is reused for
        // `ZonedDateTime`'s own property-bag path, which shares the
        // identical field set. See `temporal_read_optional_integer`'s own
        // doc comment; verified directly against `order-of-operations.js`.
        // `options` itself (the caller's `overflow` source) is deliberately
        // read *after* every field below, not before -- `ToTemporalDate`'s
        // real algorithm resolves `fields = PrepareCalendarFields(...)`
        // strictly before `resolvedOptions = GetOptionsObject(options)`
        // (`PlainDate/from/order-of-operations.js`'s own
        // `expectedOptionsReading` block comes *after*
        // `expectedOpsForPrimitiveOptions`).
        let requested_day = self.temporal_read_optional_integer(bag, "day", 1, i32::MAX)?;
        // `iso8601` has no era concept at all -- its own field-name list
        // never includes `era`/`eraYear`, so neither property is even read
        // (confirmed directly by `order-of-operations.js`'s own expected
        // sequence, which has no `era`/`eraYear` entries for an `iso8601`
        // receiver). Every other calendar's field list includes both
        // regardless of whether it individually supports eras.
        let read_era_fields = calendar != "iso8601";
        let (era, era_year) = if read_era_fields {
            let era_v = self.get_property(bag, &"era".into())?;
            let era = (!matches!(era_v, Value::Undefined))
                .then(|| self.coerce_string(&era_v))
                .transpose()?
                .map(|value| {
                    value
                        .to_utf8()
                        .map_err(|_| RuntimeError::RangeError("invalid Temporal era".into()))
                })
                .transpose()?;
            // `eraYear`'s own `Get` happens here, at its correct
            // alphabetical position; its conversion stays conditional on
            // whether `era` was actually supplied (matching the unchanged
            // value semantics below) -- no fixture in the pinned corpus
            // exercises `era`+`eraYear` together on a getter-observed
            // property bag to pin the exact relative position of
            // `eraYear`'s own coercion, so this is a narrow, documented
            // residual rather than a guess.
            let era_year = self.get_property(bag, &"eraYear".into())?;
            (era, era_year)
        } else {
            (None, Value::Undefined)
        };
        let (requested_hour, requested_microsecond, requested_millisecond, requested_minute) =
            if is_date_time {
                (
                    self.temporal_read_optional_integer(bag, "hour", 0, 23)?,
                    self.temporal_read_optional_integer(bag, "microsecond", 0, 999)?,
                    self.temporal_read_optional_integer(bag, "millisecond", 0, 999)?,
                    self.temporal_read_optional_integer(bag, "minute", 0, 59)?,
                )
            } else {
                (None, None, None, None)
            };
        let requested_month = self.temporal_read_optional_integer(bag, "month", 1, 99)?;
        let month_code =
            self.temporal_read_optional_string(bag, "monthCode", "invalid Temporal month code")?;
        // A leap second (`60`) is always constrained to `59`, matching the
        // ISO-string grammar's own `:60` handling (`iso.rs`'s
        // `parse_time_spec`) — Temporal has no internal leap-second
        // representation, so a property bag's `second: 60` must be
        // tolerated the same way rather than rejected outright
        // (`relativeto-leap-second.js`, reached via `Temporal.Duration`'s
        // own `relativeTo` property-bag path, which -- unlike a bare
        // `PlainDate` bag -- now reads this field too). The `.min(59)`
        // clamp is applied once the field is actually used, below.
        let (requested_nanosecond, requested_second) = if is_date_time {
            (
                self.temporal_read_optional_integer(bag, "nanosecond", 0, 999)?,
                self.temporal_read_optional_integer(bag, "second", 0, 60)?,
            )
        } else {
            (None, None)
        };
        // `-9_999..=9_999` was too narrow: a `PlainDate`/`PlainDateTime`/
        // `ZonedDateTime` property bag's `year` field is a plain integer
        // with no bound of its own (`ToIntegerWithTruncation` doesn't clamp
        // it) — the *real* representable-range check happens afterward,
        // once an actual calendar date exists
        // (`epoch::is_date_within_limits`/`is_date_time_within_limits`).
        // `relativeto-date-limits.js`'s extreme-year property bags (the
        // exact `-271821`/`275760` boundary) are what surfaced this —
        // reached via `Temporal.Duration`'s own `relativeTo` reuse of this
        // function, though the same bound applied to every other caller too.
        let requested_year = self.temporal_read_optional_integer(bag, "year", -275_760, 275_760)?;

        let mut fields = DateFields::default();
        if let Some(era) = era.as_deref() {
            fields.era = Some(era.as_bytes());
            fields.era_year = Some(self.temporal_integer(&era_year, -9_999, 9_999, "era year")?);
        } else {
            if !matches!(era_year, Value::Undefined) {
                return Err(RuntimeError::RangeError(
                    "Temporal eraYear requires an era".into(),
                ));
            }
            fields.extended_year = Some(requested_year.ok_or_else(|| {
                RuntimeError::TypeError("Temporal date fields require year".into())
            })?);
        }

        if let Some(month_code) = month_code.as_deref() {
            // Month codes preserve leap-month identity. If a property bag
            // also names `month`, validate it after calendar resolution.
            fields.month_code = Some(month_code.as_bytes());
        } else if let Some(month) = requested_month {
            fields.ordinal_month = Some(month as u8);
        } else {
            return Err(RuntimeError::TypeError(
                "Temporal date fields require month or monthCode".into(),
            ));
        }
        // `1..=i32::MAX`, not `1..=31` -- `ToPositiveIntegerWithTruncation`
        // has no upper bound at all: a raw property-bag `day` beyond a
        // month's real length must reach the calendar's own overflow-aware
        // `Date::try_from_fields` below (which throws under `"reject"` and
        // clamps under the default `"constrain"`), not be rejected here
        // before overflow ever gets a say. Every `.with()`-style call site
        // in this file already uses this exact same widened bound/cast
        // shape (e.g. `temporal_zoned_date_time_with`'s own `requested_day`).
        let requested_day = requested_day
            .ok_or_else(|| RuntimeError::TypeError("Temporal date fields require day".into()))?;
        fields.day = Some(requested_day as u8);
        // Every field above has now been read -- `overflow` is resolved
        // only now (for an `OverflowInput::Options` caller), per
        // `OverflowInput`'s own doc comment.
        let reject = match overflow {
            OverflowInput::Options(options) => {
                let resolved_options = self.temporal_options(options)?;
                self.temporal_overflow_option(&resolved_options)?
            }
            OverflowInput::Resolved(reject) => reject,
        };
        let calendar_kind = calendar::calendar_kind(&calendar)
            .expect("temporal_calendar validates the calendar identifier");
        let mut icu_options = icu_calendar::options::DateFromFieldsOptions::default();
        icu_options.overflow = Some(if reject {
            icu_calendar::options::Overflow::Reject
        } else {
            icu_calendar::options::Overflow::Constrain
        });
        let date = Date::try_from_fields(fields, icu_options, AnyCalendar::new(calendar_kind))
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar date".into()))?;
        // `year` is checked against the resolved date to catch a `year`
        // that contradicts an also-supplied `era`/`eraYear` (which is what
        // actually drives field resolution whenever an era is present — see
        // the branch above). `month` is checked only when `monthCode` was
        // *also* supplied — `monthCode` wins field resolution (the branch
        // above), so an inconsistent plain `month` alongside it must still
        // be caught as a conflict (`with/overflow.js`'s `{ month: 5,
        // monthCode: "M06" }`); but when `month` is the *only* month field
        // given, `overflow: "constrain"` (the default) legitimately
        // resolves an out-of-range one to a different value (that same
        // fixture's `{ month: 13 }` constrains to `12`), so it must not be
        // re-validated as an "inconsistency" there.
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
        let mut value = Self::temporal_value_from_calendar_date(kind, calendar, date);
        if is_date_time {
            value.hour = requested_hour.unwrap_or(0) as u8;
            value.minute = requested_minute.unwrap_or(0) as u8;
            value.second = requested_second.map(|value| value.min(59)).unwrap_or(0) as u8;
            value.millisecond = requested_millisecond.unwrap_or(0) as u16;
            value.microsecond = requested_microsecond.unwrap_or(0) as u16;
            value.nanosecond = requested_nanosecond.unwrap_or(0) as u16;
        }
        Ok(value)
    }

    pub(in super::super) fn temporal_optional_integer(
        &mut self,
        value: &Value,
        default: i32,
        minimum: i32,
        maximum: i32,
        name: &str,
    ) -> Result<i32, RuntimeError> {
        if *value == Value::Undefined {
            Ok(default)
        } else {
            self.temporal_integer(value, minimum, maximum, name)
        }
    }

    /// `PrepareCalendarFields`/`PreparePartialCalendarFields`'s own
    /// per-field shape for a numeric property: `Get`, then — only if the
    /// result is not `undefined` — immediately `ToIntegerWithTruncation`
    /// it, before moving on to the next field name. Callers that need
    /// several fields from the same object must invoke this (and
    /// `temporal_read_optional_string` below) once per field, **in the
    /// exact alphabetical order of the field names themselves**, and never
    /// batch every `Get` ahead of every conversion — the interleaving
    /// itself is observable (`order-of-operations.js`'s `"get
    /// fields.day"`/`"get fields.day.valueOf"`/`"call fields.day.valueOf"`
    /// triple appearing before the next field's own `"get fields.<next>"`).
    pub(in super::super) fn temporal_read_optional_integer(
        &mut self,
        object: &Value,
        name: &'static str,
        minimum: i32,
        maximum: i32,
    ) -> Result<Option<i32>, RuntimeError> {
        let value = self.get_property(object, &name.into())?;
        (!matches!(value, Value::Undefined))
            .then(|| self.temporal_integer(&value, minimum, maximum, name))
            .transpose()
    }

    /// The string-field counterpart of `temporal_read_optional_integer`:
    /// `Get`, then immediately `ToString` a present value, matching
    /// `order-of-operations.js`'s `"get fields.monthCode"`/`"get
    /// fields.monthCode.toString"`/`"call fields.monthCode.toString"`
    /// triple for a `monthCode`/`era` field.
    pub(in super::super) fn temporal_read_optional_string(
        &mut self,
        object: &Value,
        name: &'static str,
        error: &str,
    ) -> Result<Option<String>, RuntimeError> {
        let value = self.get_property(object, &name.into())?;
        (!matches!(value, Value::Undefined))
            .then(|| self.coerce_string(&value))
            .transpose()?
            .map(|value| {
                value
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError(error.into()))
            })
            .transpose()
    }

    /// A property bag's `offset` field: `Get`, then `ToPrimitive` with a
    /// string hint, then require an actual `String` -- a non-object,
    /// non-string primitive (`0`/`null`/`true`/`1000n`) is a `TypeError`
    /// without ever being stringified (`offset-property-invalid-string.js`),
    /// never a `RangeError` from a coerced-then-rejected string like `"0"`.
    /// Done per field, like `temporal_read_optional_string`, so the result
    /// is a plain `String` before anything else is read.
    pub(in super::super) fn temporal_read_optional_offset_string(
        &mut self,
        object: &Value,
    ) -> Result<Option<String>, RuntimeError> {
        let value = self.get_property(object, &"offset".into())?;
        if matches!(value, Value::Undefined) {
            return Ok(None);
        }
        let primitive = self.coerce_primitive(&value, "string")?;
        if !matches!(primitive, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.ZonedDateTime offset must be a string".into(),
            ));
        }
        let text = self.coerce_string(&primitive)?;
        text.to_utf8()
            .map(Some)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal offset".into()))
    }

    pub(in super::super) fn temporal_duration_integer(
        &mut self,
        value: &Value,
        name: &str,
    ) -> Result<i128, RuntimeError> {
        if *value == Value::Undefined {
            return Ok(0);
        }
        let value = self.coerce_number(value)?;
        if !value.is_finite() || value.fract() != 0.0 || value.abs() >= 2_f64.powi(100) {
            return Err(RuntimeError::RangeError(format!(
                "invalid Temporal.Duration {name}"
            )));
        }
        Ok(value as i128)
    }

    pub(in super::super) fn temporal_value_from_args(
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

    pub(in super::super) fn temporal_value_from_string(
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

    pub(in super::super) fn alloc_temporal_value(
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

    pub(in super::super) fn temporal_constructor(
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

    pub(in super::super) fn temporal_from(
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

    pub(in super::super) fn temporal_with_calendar(
        &mut self,
        receiver: &Value,
        calendar: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal.withCalendar requires a Temporal receiver".into())
        })?;
        let mut value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError("Temporal.withCalendar requires a Temporal receiver".into())
        })?;
        value.calendar = self.temporal_calendar_identifier(calendar)?;
        self.alloc_temporal_value(value, false)
    }

    /// Every Temporal prototype accessor. `native_call` has already checked the
    /// receiver against the type of the prototype the getter was installed on
    /// (`NativeFunction::temporal_receiver_kind`), so `value.kind` here is
    /// always one this getter is defined for and no arm re-checks it.
    pub(in super::super) fn temporal_getter(
        &mut self,
        receiver: &Value,
        getter: native::TemporalGetter,
    ) -> Result<Value, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal getter requires a Temporal receiver".into())
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError("Temporal getter requires a Temporal receiver".into())
        })?;
        match getter {
            native::TemporalGetter::DurationYears
            | native::TemporalGetter::DurationMonths
            | native::TemporalGetter::DurationWeeks
            | native::TemporalGetter::DurationDays
            | native::TemporalGetter::DurationHours
            | native::TemporalGetter::DurationMinutes
            | native::TemporalGetter::DurationSeconds
            | native::TemporalGetter::DurationMilliseconds
            | native::TemporalGetter::DurationMicroseconds
            | native::TemporalGetter::DurationNanoseconds => {
                let duration = value
                    .duration
                    .as_deref()
                    .expect("Temporal.Duration values retain a duration record");
                let field = match getter {
                    native::TemporalGetter::DurationYears => duration.years,
                    native::TemporalGetter::DurationMonths => duration.months,
                    native::TemporalGetter::DurationWeeks => duration.weeks,
                    native::TemporalGetter::DurationDays => duration.days,
                    native::TemporalGetter::DurationHours => duration.hours,
                    native::TemporalGetter::DurationMinutes => duration.minutes,
                    native::TemporalGetter::DurationSeconds => duration.seconds,
                    native::TemporalGetter::DurationMilliseconds => duration.milliseconds,
                    native::TemporalGetter::DurationMicroseconds => duration.microseconds,
                    native::TemporalGetter::DurationNanoseconds => duration.nanoseconds,
                    _ => unreachable!("all Temporal.Duration getters are listed above"),
                };
                Ok(Value::Number(field as f64))
            }
            native::TemporalGetter::DurationSign | native::TemporalGetter::DurationBlank => {
                let sign = value
                    .duration
                    .as_deref()
                    .expect("Temporal.Duration values retain a duration record")
                    .sign();
                Ok(if getter == native::TemporalGetter::DurationSign {
                    Value::Number(sign.into())
                } else {
                    Value::Bool(sign == 0)
                })
            }
            native::TemporalGetter::CalendarId => Ok(Value::String(value.calendar.into())),
            native::TemporalGetter::EpochMilliseconds => {
                // `floor(epochNanoseconds / 10^6)`, not a truncation toward
                // zero: a pre-epoch instant's milliseconds round *down*
                // (Test262's `epochMilliseconds/basic.js`).
                let million = BigInt::from(1_000_000);
                let mut milliseconds = &value.epoch_nanoseconds / &million;
                if &value.epoch_nanoseconds % &million != BigInt::from(0)
                    && value.epoch_nanoseconds < BigInt::from(0)
                {
                    milliseconds -= 1;
                }
                milliseconds
                    .to_f64()
                    .map(Value::Number)
                    .ok_or_else(|| RuntimeError::RangeError("invalid Temporal instant".into()))
            }
            native::TemporalGetter::EpochNanoseconds => Ok(Value::BigInt(value.epoch_nanoseconds)),
            native::TemporalGetter::TimeZoneId => Ok(Value::String(value.time_zone.into())),
            native::TemporalGetter::Hour
            | native::TemporalGetter::Minute
            | native::TemporalGetter::Second
            | native::TemporalGetter::Millisecond
            | native::TemporalGetter::Microsecond
            | native::TemporalGetter::Nanosecond => Ok(Value::Number(match getter {
                native::TemporalGetter::Hour => value.hour.into(),
                native::TemporalGetter::Minute => value.minute.into(),
                native::TemporalGetter::Second => value.second.into(),
                native::TemporalGetter::Millisecond => value.millisecond.into(),
                native::TemporalGetter::Microsecond => value.microsecond.into(),
                native::TemporalGetter::Nanosecond => value.nanosecond.into(),
                _ => unreachable!("all Temporal.PlainTime getters are listed above"),
            })),
            native::TemporalGetter::DayOfWeek
            | native::TemporalGetter::DayOfYear
            | native::TemporalGetter::WeekOfYear
            | native::TemporalGetter::YearOfWeek
            | native::TemporalGetter::DaysInWeek => {
                // Calendar-invariant: Temporal's day-of-week/week-of-year
                // getters operate on the ISO representation for every
                // calendar, per the current spec revision.
                let date = (value.year, value.month, value.day);
                Ok(match getter {
                    native::TemporalGetter::DayOfWeek => {
                        Value::Number(plain_date::iso_day_of_week(date).into())
                    }
                    native::TemporalGetter::DayOfYear => {
                        Value::Number(plain_date::iso_day_of_year(date).into())
                    }
                    native::TemporalGetter::WeekOfYear => {
                        Value::Number(plain_date::iso_week_of_year(date).0.into())
                    }
                    native::TemporalGetter::YearOfWeek => {
                        Value::Number(plain_date::iso_week_of_year(date).1.into())
                    }
                    native::TemporalGetter::DaysInWeek => Value::Number(7.0),
                    _ => unreachable!("all ISO week-date getters are listed above"),
                })
            }
            native::TemporalGetter::OffsetNanoseconds | native::TemporalGetter::Offset => {
                let zone = temporal_zoned_date_time_zone(&value);
                let offset = zone.offset_nanoseconds_for(&value.epoch_nanoseconds);
                Ok(if getter == native::TemporalGetter::OffsetNanoseconds {
                    Value::Number(offset as f64)
                } else {
                    Value::String(format_offset_nanoseconds_exact(offset).into())
                })
            }
            native::TemporalGetter::HoursInDay => {
                let zone = temporal_zoned_date_time_zone(&value);
                let date = (value.year, value.month, value.day);
                let length = zoned_date_time::day_length_nanoseconds(&zone, date);
                Ok(Value::Number(length as f64 / 3_600_000_000_000.0))
            }
            getter => {
                let fields = self.temporal_calendar_fields(&value)?;
                match getter {
                    native::TemporalGetter::Year => Ok(Value::Number(fields.year.into())),
                    native::TemporalGetter::Month => Ok(Value::Number(fields.month.into())),
                    native::TemporalGetter::MonthCode => {
                        Ok(Value::String(fields.month_code.into()))
                    }
                    native::TemporalGetter::Day => Ok(Value::Number(fields.day.into())),
                    native::TemporalGetter::Era => Ok(fields
                        .era
                        .map_or(Value::Undefined, |era| Value::String(era.into()))),
                    native::TemporalGetter::EraYear => Ok(fields
                        .era_year
                        .map_or(Value::Undefined, |year| Value::Number(year.into()))),
                    native::TemporalGetter::MonthsInYear => {
                        Ok(Value::Number(fields.months_in_year.into()))
                    }
                    native::TemporalGetter::DaysInMonth => {
                        Ok(Value::Number(fields.days_in_month.into()))
                    }
                    native::TemporalGetter::DaysInYear => {
                        Ok(Value::Number(fields.days_in_year.into()))
                    }
                    native::TemporalGetter::InLeapYear => Ok(Value::Bool(fields.in_leap_year)),
                    _ => unreachable!("every other Temporal getter is handled above"),
                }
            }
        }
    }

    pub(in super::super) fn temporal_plain_to_zoned_date_time(
        &mut self,
        receiver: &Value,
        time_zone: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Temporal.toZonedDateTime requires a plain receiver".into())
        })?;
        let mut value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError("Temporal.toZonedDateTime requires a plain receiver".into())
        })?;
        if !matches!(
            value.kind,
            TemporalKind::PlainDate | TemporalKind::PlainDateTime
        ) {
            return Err(RuntimeError::TypeError(
                "Temporal.toZonedDateTime requires a plain receiver".into(),
            ));
        }
        // `Temporal.PlainDate.prototype.toZonedDateTime` takes one `item`
        // argument (a bare identifier or a `{ timeZone, plainTime }` bag) and
        // no options object; `Temporal.PlainDateTime.prototype` takes a bare
        // identifier plus an options object carrying `disambiguation`.
        let (zone, time, disambiguation) = if value.kind == TemporalKind::PlainDateTime {
            let zone = self.temporal_time_zone(time_zone)?;
            let disambiguation = self.temporal_disambiguation(options)?;
            (zone, None, disambiguation)
        } else {
            let (zone, time) = self.temporal_plain_date_zone_and_time(time_zone)?;
            (zone, time, time_zone::Disambiguation::Compatible)
        };
        value.epoch_nanoseconds = if value.kind == TemporalKind::PlainDate && time.is_none() {
            // A `PlainDate` with no time of day becomes the zone's start of
            // day, which is not always local midnight.
            zone.start_of_day((value.year, value.month, value.day))
        } else {
            if let Some((hour, minute, second, millisecond, microsecond, nanosecond)) = time {
                value.hour = hour;
                value.minute = minute;
                value.second = second;
                value.millisecond = millisecond;
                value.microsecond = microsecond;
                value.nanosecond = nanosecond;
            }
            zone.epoch_nanoseconds_for(
                (value.year, value.month, value.day),
                (
                    value.hour,
                    value.minute,
                    value.second,
                    value.millisecond,
                    value.microsecond,
                    value.nanosecond,
                ),
                disambiguation,
            )
            .map_err(temporal_resolution_error)?
        };
        if !epoch::is_in_instant_range(&value.epoch_nanoseconds) {
            return Err(RuntimeError::RangeError(
                "Temporal.ZonedDateTime epoch nanoseconds are outside the supported range".into(),
            ));
        }
        // The stored ISO fields are the *resolved* local wall-clock ones, not
        // the requested ones: a skipped local time resolves to the shifted
        // time, and a start-of-day resolution to the zone's real first
        // wall-clock time of the day.
        temporal_set_local_fields(&mut value, &zone);
        value.kind = TemporalKind::ZonedDateTime;
        value.time_zone = zone.identifier();
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.ZonedDateTime.prototype.toLocaleString`'s own
    /// `GetDateTimeFormat`/`TemporalObjectToLocaleString` shape (per the
    /// Temporal-in-`Intl` proposal, ported from Gecko's
    /// `DateTimeFormat.cpp`), which is genuinely different from the plain
    /// `Intl.DateTimeFormat` constructor path every other
    /// `temporal_*_to_locale_string` uses via `create_date_time_format`:
    ///
    /// 1. A `timeZone` option is rejected unconditionally, *even if its
    ///    value agrees with the receiver's own zone* -- `options` must not
    ///    have a `timeZone` property at all, per `CreateDateTimeFormat`'s own
    ///    "steps 15-17" (`toLocaleStringTimeZone` present -> throw before
    ///    ever coercing the option's value). `date_time_format_options`'s own
    ///    `string_option` call already treats an absent-or-`undefined`
    ///    property as `None`, so `time_zone.is_some()` here is exactly that
    ///    check (`toLocaleString/options-timeZone.js`).
    /// 2. Once no `dateStyle`/`timeStyle` and no individual date/time
    ///    component was requested at all, the *default* field set is
    ///    year/month/day/hour/minute/second (numeric) **plus**
    ///    `timeZoneName: "short"` -- `GetDateTimeFormat`'s own
    ///    `Defaults::ZonedDateTime` (distinct from `Defaults::All`, which
    ///    every other Temporal type's own defaulting uses and which never
    ///    adds a time zone name). This is the one piece a bare
    ///    `Intl.DateTimeFormat` construction has no way to express, since it
    ///    has no receiver-derived zone to name by default
    ///    (`default-includes-time-and-time-zone-name.js`,
    ///    `options-undefined.js`, `locales-undefined.js`,
    ///    `dateStyle-timeStyle-undefined.js`, `hourcycle.js`). Any single
    ///    explicit component (including a lone `timeZoneName`) still skips
    ///    the whole default set, matching `Date.prototype.toLocaleString`'s
    ///    own lone-option behavior (`lone-options-accepted.js`).
    pub(in super::super) fn temporal_zoned_date_time_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
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
        let milliseconds = (&value.epoch_nanoseconds / 1_000_000_u32)
            .to_f64()
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal instant".into()))?;
        let stack_base = self.stack.len();
        let result = (|| {
            let mut options = self.date_time_format_options(native::argument(args, 1))?;
            if options.time_zone.is_some() {
                return Err(RuntimeError::TypeError(
                    "Temporal.ZonedDateTime.prototype.toLocaleString does not accept a timeZone option"
                        .into(),
                ));
            }
            options.time_zone = Some(value.time_zone.to_string());
            // `era` and `timeZoneName` are deliberately excluded from this
            // gate (mirroring Gecko's own `anyPresent`/`requiredOptions`
            // check, which the same two fields are excluded from): a lone
            // `{ timeZoneName: "short" }` with no other component must still
            // get the full date+time default set alongside it, not just the
            // time zone name by itself (`toLocaleString/
            // lone-options-accepted.js`'s own `timeZoneName` case, verified
            // against `Date.prototype.toLocaleString`'s identical exclusion
            // for the plain, non-Temporal `required=Any, defaults=All` case
            // this mirrors).
            let needs_defaults = options.date_style.is_none()
                && options.time_style.is_none()
                && options.weekday.is_none()
                && options.year.is_none()
                && options.month.is_none()
                && options.day.is_none()
                && options.day_period.is_none()
                && options.hour.is_none()
                && options.minute.is_none()
                && options.second.is_none()
                && options.fractional_second_digits.is_none();
            if needs_defaults {
                options.year = Some(blueice_ecma402::DateTimeWidth::Numeric);
                options.month = Some(blueice_ecma402::DateTimeWidth::Numeric);
                options.day = Some(blueice_ecma402::DateTimeWidth::Numeric);
                options.hour = Some(blueice_ecma402::DateTimeWidth::Numeric);
                options.minute = Some(blueice_ecma402::DateTimeWidth::Numeric);
                options.second = Some(blueice_ecma402::DateTimeWidth::Numeric);
                if options.time_zone_name.is_none() {
                    options.time_zone_name = Some("short".to_string());
                }
            }
            let locales = self.canonical_locales(native::argument(args, 0))?;
            blueice_ecma402::DateTimeFormat::try_new(&locales, options)
                .and_then(|format| format.format(milliseconds))
                .map(|formatted| Value::String(formatted.into()))
                .map_err(|error| RuntimeError::RangeError(error.to_string()))
        })();
        self.stack.truncate(stack_base);
        result
    }

    // ---- Stage 1 Track C: Temporal.Instant arithmetic -------------------

    /// `GetOptionsObject`: `undefined` becomes a fresh empty object; an
    /// Object is used as-is; any other value is a `TypeError` — it is
    /// deliberately *not* boxed through `ToObject`, so
    /// `instant.toString("some string")` throws rather than reading options
    /// off a String wrapper. Test262's
    /// `Instant/prototype/toString/options-wrong-type.js` and
    /// `PlainTime/prototype/until/options-wrong-type.js` both pass
    /// `"hello"`/`1`/`1n` and require the throw.
    pub(in super::super) fn temporal_options(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let result = match value {
            Value::Undefined => Value::Object(self.with_roots(|heap| heap.alloc_object(None))?),
            Value::Object(_) => value.clone(),
            _ => {
                return Err(RuntimeError::TypeError(
                    "Temporal options must be an object or undefined".into(),
                ));
            }
        };
        self.stack.push(result.clone());
        Ok(result)
    }

    /// The `roundTo` parameter of `Temporal.Instant.prototype.round`: a
    /// String is shorthand for `{ smallestUnit: <string> }`, carried on a
    /// null-prototype object so `Object.prototype` accessors for the other
    /// option names are never consulted (Test262's
    /// `string-shorthand-no-object-prototype-pollution.js`).
    pub(in super::super) fn temporal_round_to(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        if *value == Value::Undefined {
            return Err(RuntimeError::TypeError(
                "Temporal.Instant.round requires a roundTo argument".into(),
            ));
        }
        if !matches!(value, Value::String(_)) {
            return self.temporal_options(value);
        }
        let object = self.with_roots(|heap| heap.alloc_object(None))?;
        let result = Value::Object(object);
        self.stack.push(result.clone());
        self.define_data(object, "smallestUnit", value.clone(), true, true, true)?;
        Ok(result)
    }

    pub(in super::super) fn temporal_string_option(
        &mut self,
        options: &Value,
        name: &str,
        allowed: &[&str],
    ) -> Result<Option<String>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let string = self.coerce_string(&value)?;
        let string = string
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError(format!("invalid {name} option")))?;
        if !allowed.is_empty() && !allowed.contains(&string.as_str()) {
            return Err(RuntimeError::RangeError(format!("invalid {name} option")));
        }
        Ok(Some(string))
    }

    /// `ToTemporalRoundingIncrement`: an integer in `1..=1e9`, default `1`.
    pub(in super::super) fn temporal_rounding_increment(
        &mut self,
        options: &Value,
    ) -> Result<i128, RuntimeError> {
        let value = self.get_property(options, &"roundingIncrement".into())?;
        if value == Value::Undefined {
            return Ok(1);
        }
        let value = self.coerce_number(&value)?;
        if !value.is_finite() {
            return Err(RuntimeError::RangeError("invalid roundingIncrement".into()));
        }
        let integer = value.trunc();
        if !(1.0..=1_000_000_000.0).contains(&integer) {
            return Err(RuntimeError::RangeError("invalid roundingIncrement".into()));
        }
        Ok(integer as i128)
    }

    pub(in super::super) fn temporal_rounding_mode(
        &mut self,
        options: &Value,
        default: blueice_ecma402::NumberRoundingMode,
    ) -> Result<blueice_ecma402::NumberRoundingMode, RuntimeError> {
        match self.temporal_string_option(
            options,
            "roundingMode",
            &[
                "ceil",
                "floor",
                "expand",
                "trunc",
                "halfCeil",
                "halfFloor",
                "halfExpand",
                "halfTrunc",
                "halfEven",
            ],
        )? {
            None => Ok(default),
            Some(mode) => Ok(rounding::parse_rounding_mode(&mode)
                .expect("temporal_string_option already validated the rounding mode name")),
        }
    }

    /// `GetTemporalUnitValuedOption`: reads a unit-valued option, accepting
    /// **every** unit name (including the calendar units `Temporal.Instant`
    /// itself never allows) plus an optional extra literal such as `"auto"`.
    /// Rejecting a syntactically valid but operation-inappropriate unit is a
    /// separate, later step — the ordering Test262's
    /// `options-read-before-algorithmic-validation.js` fixtures observe.
    pub(in super::super) fn temporal_unit_option(
        &mut self,
        options: &Value,
        name: &str,
        auto: bool,
    ) -> Result<Option<rounding::Unit>, RuntimeError> {
        let value = self.get_property(options, &name.into())?;
        if value == Value::Undefined {
            return Ok(None);
        }
        let string = self.coerce_string(&value)?;
        let string = string
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError(format!("invalid {name} option")))?;
        if auto && string == "auto" {
            return Ok(None);
        }
        rounding::parse_unit(&string)
            .map(Some)
            .ok_or_else(|| RuntimeError::RangeError(format!("invalid {name} option")))
    }

    /// Narrows an already-read unit option to a time unit, optionally also
    /// rejecting `"hour"` (which `toString` disallows while `round` allows).
    pub(in super::super) fn temporal_time_unit(
        unit: Option<rounding::Unit>,
        name: &str,
        allow_hour: bool,
    ) -> Result<Option<rounding::TimeUnit>, RuntimeError> {
        match unit {
            None => Ok(None),
            Some(rounding::Unit::Time(rounding::TimeUnit::Hour)) if !allow_hour => {
                Err(RuntimeError::RangeError(format!("invalid {name} option")))
            }
            Some(rounding::Unit::Time(unit)) => Ok(Some(unit)),
            Some(rounding::Unit::Date(_)) => {
                Err(RuntimeError::RangeError(format!("invalid {name} option")))
            }
        }
    }
}
