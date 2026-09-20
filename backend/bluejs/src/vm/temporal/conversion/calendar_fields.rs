// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Calendar identifiers and calendar-field resolution: `ToTemporalCalendarIdentifier`, the
//! ICU-backed field derivation behind the calendar getters, and building a plain date
//! from a property bag's calendar fields.

use super::super::*;

impl Vm {
    /// The raw `Temporal.PlainDate`/`PlainDateTime`/etc. **constructor**'s
    /// own positional `calendar` argument: a bare calendar ID string only.
    /// Test262's `calendar-invalid-iso-string.js` confirms a full
    /// date-with-annotation string (`"1997-12-04[u-ca=iso8601]"`) is a
    /// `RangeError` here specifically, unlike [`Self::temporal_calendar`]'s
    /// wider grammar below.
    pub(in super::super::super) fn temporal_calendar(
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
    pub(in super::super::super) fn temporal_calendar_identifier(
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

    pub(in super::super::super) fn temporal_calendar_fields(
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

    pub(in super::super::super) fn temporal_value_from_calendar_date(
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

    pub(in super::super::super) fn temporal_plain_date_from_fields(
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

    pub(in super::super::super) fn temporal_with_calendar(
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
}
