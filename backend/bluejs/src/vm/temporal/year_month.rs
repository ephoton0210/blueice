// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

/// Phase 26 Stage 2's second `PlainYearMonth`/`PlainMonthDay` slice. Kept as
/// its own `impl Vm` block (rather than folded into the block above) so this
/// addition stays textually disjoint from the region a sibling worktree is
/// concurrently editing for `PlainDate`/`PlainDateTime` bug fixes -- per this
/// document's own repeatedly-recorded "git diff misalignment" merge pattern.
impl Vm {
    /// Brand check shared by every `Temporal.PlainYearMonth` prototype
    /// method, mirroring `temporal_date_receiver`'s own pattern for
    /// `PlainDate`/`PlainDateTime`.
    pub(in super::super) fn temporal_year_month_receiver(
        &mut self,
        receiver: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainYearMonth method requires a matching receiver".into(),
            )
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainYearMonth method requires a matching receiver".into(),
            )
        })?;
        if value.kind != TemporalKind::PlainYearMonth {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainYearMonth method requires a matching receiver".into(),
            ));
        }
        Ok(value)
    }

    /// Same as [`Self::temporal_year_month_receiver`], for `PlainMonthDay`.
    pub(in super::super) fn temporal_month_day_receiver(
        &mut self,
        receiver: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainMonthDay method requires a matching receiver".into(),
            )
        })?;
        let value = self.heap.temporal_value(object)?.ok_or_else(|| {
            RuntimeError::TypeError(
                "Temporal.PlainMonthDay method requires a matching receiver".into(),
            )
        })?;
        if value.kind != TemporalKind::PlainMonthDay {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay method requires a matching receiver".into(),
            ));
        }
        Ok(value)
    }

    /// `CalendarYearMonthFromFields`'s property-bag entry point --
    /// `Temporal.PlainYearMonth.from({...})` and the object branch of
    /// `ToTemporalYearMonth`.
    pub(in super::super) fn temporal_plain_year_month_from_fields(
        &mut self,
        bag: &Value,
        overflow: OverflowInput,
    ) -> Result<TemporalValue, RuntimeError> {
        let calendar_value = self.get_property(bag, &"calendar".into())?;
        let calendar = self.temporal_calendar_identifier(&calendar_value)?;

        // `PrepareCalendarFields` reads and immediately coerces every
        // recognized property in strict alphabetical order -- `era`,
        // `eraYear` (only for a calendar that has eras, matching
        // `temporal_plain_date_from_fields`'s identical gate), `month`,
        // `monthCode`, `year` -- interleaved with each field's own
        // immediate conversion. `options`/`overflow` are deliberately
        // resolved only after every field below, per `OverflowInput`'s own
        // doc comment. Verified directly against `order-of-operations.js`.
        let read_era_fields = calendar::calendar_supports_era(&calendar);
        let era_v = if read_era_fields {
            self.get_property(bag, &"era".into())?
        } else {
            Value::Undefined
        };
        let era_s = (!matches!(era_v, Value::Undefined))
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
        // `era` and `eraYear` only mean something together (a `TypeError`,
        // like any other missing field, ahead of every range check --
        // `one-of-era-erayear-undefined.js`).
        if era_s.is_some() != !matches!(era_year_v, Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "Temporal era and eraYear must be supplied together".into(),
            ));
        }
        let era_year_num = era_s
            .is_some()
            .then(|| self.temporal_integer(&era_year_v, i32::MIN, i32::MAX, "era year"))
            .transpose()?;
        // `ToPositiveIntegerWithTruncation`: no upper bound. An ordinal `month`
        // past the year's last is constrained (or rejected) by the calendar,
        // not by this read, and the cast to `u8` saturates rather than wraps
        // (`from/overflow-constrain.js`'s `month: 99999`).
        let requested_month = self.temporal_read_optional_integer(bag, "month", 1, i32::MAX)?;
        let month_code_s =
            self.temporal_read_optional_string(bag, "monthCode", "invalid Temporal month code")?;
        // Unbounded at the field-reading stage (`ToIntegerWithTruncation`),
        // matching `temporal_year_month_with`'s own identical fix's doc
        // comment -- `iso::is_year_month_within_limits` below still
        // range-checks the resolved date.
        let requested_year =
            self.temporal_read_optional_integer(bag, "year", i32::MIN, i32::MAX)?;

        // Every field has now been read, in alphabetical order. The
        // *validation* order below is deliberately **not** alphabetical,
        // though: `year`'s own required-field check runs before `month`'s,
        // matching `CalendarResolveFields`'s own fixed validation order --
        // confirmed directly by `missing-properties.js`'s own "year should
        // be checked after fetching but before resolving the month"
        // comment (a bag with getters for `month`/`monthCode` but no
        // `year` at all must still fire both of those getters, per the
        // alphabetical read order above, before throwing the `year`
        // `TypeError` first).
        if era_s.is_none() && requested_year.is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainYearMonth fields require year".into(),
            ));
        }
        if month_code_s.is_none() && requested_month.is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainYearMonth fields require month or monthCode".into(),
            ));
        }

        let reject = match overflow {
            OverflowInput::Options(options) => {
                let resolved_options = self.temporal_options(options)?;
                self.temporal_overflow_option(&resolved_options)?
            }
            OverflowInput::Resolved(reject) => reject,
        };
        let calendar_kind = calendar::calendar_kind(&calendar)
            .expect("temporal_calendar_identifier validates the calendar identifier");
        let fields = plain_year_month::YearMonthFields {
            era: era_s.as_deref(),
            era_year: era_year_num,
            extended_year: requested_year,
            month_code: month_code_s.as_deref(),
            ordinal_month: requested_month.map(|value| value.min(i32::from(u8::MAX)) as u8),
        };
        let date = plain_year_month::year_month_from_fields(calendar_kind, &fields, reject)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar year-month".into()))?;
        if !iso::is_year_month_within_limits(date.0, date.1) {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainYearMonth is outside the supported range".into(),
            ));
        }
        Ok(Self::temporal_date_value(
            TemporalKind::PlainYearMonth,
            calendar,
            date,
        ))
    }

    /// `CalendarMonthDayFromFields`'s property-bag entry point --
    /// `Temporal.PlainMonthDay.from({...})` and the object branch of
    /// `ToTemporalMonthDay`.
    pub(in super::super) fn temporal_plain_month_day_from_fields(
        &mut self,
        bag: &Value,
        overflow: OverflowInput<'_>,
    ) -> Result<TemporalValue, RuntimeError> {
        let calendar_value = self.get_property(bag, &"calendar".into())?;
        let calendar = self.temporal_calendar_identifier(&calendar_value)?;
        let supports_era = calendar::calendar_supports_era(&calendar);

        // `PrepareCalendarFields` reads and converts one field at a time, in
        // alphabetical order -- `day`, `era`, `eraYear`, `month`,
        // `monthCode`, `year` -- so each `Get` is followed by its own
        // conversion before the next field is read (`from/
        // order-of-operations.js`). Converting immediately also means no
        // object-valued `Get` result is held in a Rust local across a later
        // call that can allocate: `propertyBagObserver` hands back a fresh
        // converting object per read, and once a collection ran, an earlier
        // unrooted one was gone before its own conversion.
        //
        // `ToPositiveIntegerWithTruncation`: only a lower bound of `1`, no
        // upper bound -- the calendar's own `overflow` regulation constrains
        // or rejects an out-of-month-range `day`, not this read.
        let requested_day = self.temporal_read_optional_integer(bag, "day", 1, i32::MAX)?;
        // `CalendarExtraFields`: requesting `Year` also reads `era`/`eraYear`
        // for a calendar that supports eras (`iso8601`/`chinese`/`dangi`
        // never do). `intl402/Temporal/PlainMonthDay/prototype/{equals,
        // toPlainDate}/infinity-throws-rangeerror.js`'s `eraYear: Infinity`
        // on a `"gregory"` receiver needs its finiteness check to run.
        let (era_s, requested_era_year) = if supports_era {
            let era_s = self.temporal_read_optional_string(bag, "era", "invalid Temporal era")?;
            let requested_era_year =
                self.temporal_read_optional_integer(bag, "eraYear", i32::MIN, i32::MAX)?;
            (era_s, requested_era_year)
        } else {
            (None, None)
        };
        // Only a lower bound of `1`: `from/overflow.js`'s `{ month: 999999 }`
        // under `overflow: "constrain"` must succeed as `M12`, not throw here.
        let requested_month = self.temporal_read_optional_integer(bag, "month", 1, i32::MAX)?;
        // `monthCode`'s own syntax is checked as soon as it is converted,
        // ahead of `year`'s conversion: `from/monthcode-invalid.js`'s
        // Symbol-`year` cases need a malformed code (`"L99M"`) to throw
        // `RangeError` before `year` is ever converted (`TypeError`), while
        // a well-formed-but-unsuitable one (`"M99L"`, judged later, once a
        // calendar resolution is attempted) lets `year`'s `TypeError` win.
        let month_code_s =
            self.temporal_read_optional_string(bag, "monthCode", "invalid Temporal month code")?;
        if let Some(code) = month_code_s.as_deref() {
            if !plain_month_day::is_well_formed_month_code(code) {
                return Err(RuntimeError::RangeError(
                    "invalid Temporal month code".into(),
                ));
            }
        }
        // `ToIntegerWithTruncation`, unbounded here: `epoch::is_date_within_limits`
        // below still range-checks the resolved date.
        let requested_year =
            self.temporal_read_optional_integer(bag, "year", i32::MIN, i32::MAX)?;

        if month_code_s.is_none() && requested_month.is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay fields require month or monthCode".into(),
            ));
        }
        if era_s.is_some() != requested_era_year.is_some() {
            return Err(RuntimeError::TypeError(
                "Temporal era and eraYear must be supplied together".into(),
            ));
        }
        // An ordinal `month`'s identity varies by year, so a **non-ISO**
        // calendar needs a `year` (or `era`+`eraYear`) whenever a `month` is
        // given -- even next to a `monthCode` that would identify the month on
        // its own (`calendarresolvefields-error-ordering-*.js`'s "Missing year
        // (required for month)"). Gecko's `CalendarResolveFields` gives `iso8601`
        // a narrower branch that needs just `day` and (`monthCode` or `month`),
        // since its reference year (1972) is fixed. Pinned by
        // `PlainMonthDay/prototype/equals/basic.js`'s bare `{ month: 1, day: 22 }`.
        if calendar != "iso8601"
            && requested_month.is_some()
            && requested_year.is_none()
            && era_s.is_none()
        {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay fields require year when month is given".into(),
            ));
        }
        // Every missing-field `TypeError` (this one, and the two above) comes
        // before any `RangeError` -- a `monthCode`/`month` conflict or a day out
        // of range only surfaces once the fields are complete.
        let day_num = requested_day.ok_or_else(|| {
            RuntimeError::TypeError("Temporal.PlainMonthDay fields require day".into())
        })?;
        let day_num_u8 = day_num.min(i32::from(u8::MAX)) as u8;
        // `GetTemporalOverflowOption` runs only after every field has been
        // read, so a primitive `options` still lets the fields be observed
        // first (`from/order-of-operations.js`).
        let reject = match overflow {
            OverflowInput::Options(options) => {
                let resolved_options = self.temporal_options(options)?;
                self.temporal_overflow_option(&resolved_options)?
            }
            OverflowInput::Resolved(reject) => reject,
        };

        let calendar_kind = calendar::calendar_kind(&calendar)
            .expect("temporal_calendar_identifier validates the calendar identifier");
        // `CalendarISOToDate`'s ISO-specific branch (`Calendar.cpp`): a
        // supplied `year` regulates the resolved `day` (e.g. whether 29
        // February constrains/rejects) but never survives into the result
        // -- the `iso8601` calendar's `PlainMonthDay` always reports the
        // fixed reference year 1972. Handled by a dedicated pure-Rust fast
        // path (`iso_month_day_from_fields`) rather than `icu_calendar`,
        // whose own internal year-range limits are far narrower than the
        // regulation year's legitimate domain here (an arbitrarily large or
        // small `year` is valid input purely for leap-year determination).
        // Pinned by `PlainMonthDay/from/iso-year-used-only-for-overflow.js`.
        let date = if calendar == "iso8601" {
            // A well-formed `monthCode` (already syntax-checked above) must
            // still denote an actual ISO 8601 month (`01`-`12`, never a
            // leap-month `L` suffix -- the ISO calendar has no leap months
            // at all) *regardless of `overflow`*: this is a suitability
            // check on the code's own meaning, not a numeric-field
            // constrain/reject regulation, so `{ monthCode: "M19" }` is a
            // `RangeError` even under the default `overflow: "constrain"`
            // -- `from/monthcode-invalid.js`'s `M00`/`M19`/`M99`/`M13`/
            // `M00L`/`M05L`/`M13L` cases.
            let month_code_ordinal = month_code_s
                .as_deref()
                .map(|code| {
                    plain_month_day::iso_month_code_ordinal(code).ok_or_else(|| {
                        RuntimeError::RangeError(
                            "monthCode is not valid for the ISO 8601 calendar".into(),
                        )
                    })
                })
                .transpose()?;
            // A `month`/`monthCode` pair that disagree is always a
            // `RangeError`, independent of `overflow` -- `from/
            // monthcode-invalid.js`'s `{ month: 12, monthCode: "M11" }`
            // ("monthCode and month conflict").
            if let (Some(month_num), Some(code_num)) = (requested_month, month_code_ordinal) {
                if month_num != i32::from(code_num) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.PlainMonthDay month and monthCode conflict".into(),
                    ));
                }
            }
            // Same saturating-cast rationale as `ordinal_month_for_fields`
            // above.
            let month = requested_month
                .map(|value| value.min(i32::from(u8::MAX)) as u8)
                .or(month_code_ordinal)
                .ok_or_else(|| RuntimeError::RangeError("invalid Temporal month code".into()))?;
            plain_month_day::iso_month_day_from_fields(
                month,
                day_num_u8,
                requested_year.unwrap_or(1972),
                reject,
            )
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar month-day".into()))?
        } else {
            // `era`/`eraYear` (supplied together) resolve the year on their
            // own via `icu_calendar`'s era-aware `Date::try_from_fields`; a
            // `year`, and an ordinal `month` next to a `monthCode`, are passed
            // along too and cross-checked against the result
            // (`fields-overspecified.js`).
            let fields = plain_month_day::MonthDayFields {
                extended_year: requested_year,
                era: era_s.as_deref().map(str::as_bytes),
                era_year: requested_era_year,
                month_code: month_code_s.as_deref(),
                ordinal_month: requested_month.map(|value| value.min(i32::from(u8::MAX)) as u8),
                day: day_num_u8,
            };
            plain_month_day::month_day_from_fields(calendar_kind, &fields, reject).map_err(
                |_| RuntimeError::RangeError("invalid Temporal calendar month-day".into()),
            )?
        };
        if !epoch::is_date_within_limits(date) {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainMonthDay is outside the supported range".into(),
            ));
        }
        Ok(Self::temporal_date_value(
            TemporalKind::PlainMonthDay,
            calendar,
            date,
        ))
    }

    /// `ToTemporalYearMonth`.
    pub(in super::super) fn temporal_to_plain_year_month(
        &mut self,
        value: &Value,
        options: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::PlainYearMonth {
                    let resolved_options = self.temporal_options(options)?;
                    self.temporal_overflow_option(&resolved_options)?;
                    return Ok(temporal);
                }
            }
            // `options` is passed through unread here -- see
            // `temporal_plain_year_month_from_fields`'s own doc comment.
            return self
                .temporal_plain_year_month_from_fields(value, OverflowInput::Options(options));
        }
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainYearMonth-like value must be an object or a string".into(),
            ));
        }
        let source = self.coerce_string(value)?.to_utf8().map_err(|_| {
            RuntimeError::RangeError("invalid Temporal.PlainYearMonth string".into())
        })?;
        let resolved_options = self.temporal_options(options)?;
        self.temporal_overflow_option(&resolved_options)?;
        let parsed = self.temporal_value_from_string(TemporalKind::PlainYearMonth, &source)?;
        if parsed.calendar == "iso8601" {
            return Ok(parsed);
        }
        let fields = self.temporal_calendar_fields(&parsed)?;
        let calendar_kind = calendar::calendar_kind(&parsed.calendar)
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
        Ok(Self::temporal_date_value(
            TemporalKind::PlainYearMonth,
            parsed.calendar,
            date,
        ))
    }

    /// `ToTemporalMonthDay`.
    pub(in super::super) fn temporal_to_plain_month_day(
        &mut self,
        value: &Value,
        options: &Value,
    ) -> Result<TemporalValue, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == TemporalKind::PlainMonthDay {
                    let resolved_options = self.temporal_options(options)?;
                    self.temporal_overflow_option(&resolved_options)?;
                    return Ok(temporal);
                }
            }
            return self
                .temporal_plain_month_day_from_fields(value, OverflowInput::Options(options));
        }
        if !matches!(value, Value::String(_)) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay-like value must be an object or a string".into(),
            ));
        }
        let source = self.coerce_string(value)?.to_utf8().map_err(|_| {
            RuntimeError::RangeError("invalid Temporal.PlainMonthDay string".into())
        })?;
        // `ToTemporalMonthDay`'s real algorithm parses the string (throwing
        // `RangeError` for a malformed one) strictly before it ever reads
        // the `overflow` option -- pinned by `from/options-wrong-type.js`'s
        // "Invalid string string processed before throwing TypeError" case,
        // an invalid string must report `RangeError` even when `options`
        // itself is a wrong-type value that would otherwise throw
        // `TypeError`.
        let parsed = self.temporal_value_from_string(TemporalKind::PlainMonthDay, &source)?;
        let resolved_options = self.temporal_options(options)?;
        self.temporal_overflow_option(&resolved_options)?;
        if parsed.calendar == "iso8601" {
            return Ok(parsed);
        }
        let fields = self.temporal_calendar_fields(&parsed)?;
        let calendar_kind = calendar::calendar_kind(&parsed.calendar)
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
        Ok(Self::temporal_date_value(
            TemporalKind::PlainMonthDay,
            parsed.calendar,
            date,
        ))
    }

    /// `Temporal.PlainYearMonth.prototype.with`. Unlike
    /// `PlainDate`/`PlainDateTime.prototype.with`, only `year`/`month`/
    /// `monthCode` are recognized overrides -- `era`/`eraYear` are always
    /// carried through unchanged from the receiver (Gecko's own
    /// `PlainYearMonth_with` restricts `PreparePartialCalendarFields` to
    /// exactly this trio).
    pub(in super::super) fn temporal_year_month_with(
        &mut self,
        receiver: &Value,
        like: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_year_month_receiver(receiver)?;
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
        let base = self.temporal_calendar_fields(&existing)?;

        // `PreparePartialCalendarFields` reads and immediately coerces
        // every recognized property in strict alphabetical order -- `era`,
        // `eraYear`, `month`, `monthCode`, `year` -- interleaved with each
        // field's own immediate conversion, and strictly before
        // `options`/`overflow` are ever read. `iso8601` has no era concept
        // at all, so neither `era` nor `eraYear` is even read for it
        // (confirmed directly by `order-of-operations.js`'s own expected
        // sequence, which has no `era`/`eraYear` entries for an `iso8601`
        // receiver) -- `chinese`/`dangi` still need to *see* a supplied
        // `era`/`eraYear` in order to reject it below.
        let read_era_fields = existing.calendar != "iso8601";
        let (era_s, requested_era_year) = if read_era_fields {
            (
                self.temporal_read_optional_string(like, "era", "invalid Temporal era")?,
                self.temporal_read_optional_integer(like, "eraYear", i32::MIN, i32::MAX)?,
            )
        } else {
            (None, None)
        };
        let requested_month = self.temporal_read_optional_integer(like, "month", 1, i32::MAX)?;
        let month_code_s =
            self.temporal_read_optional_string(like, "monthCode", "invalid Temporal month code")?;
        // `ToIntegerWithTruncation`: unbounded at the field-reading stage
        // for `year` (`CalendarFields.cpp`'s `CalendarField::Year` case)
        // -- the real representable-range check happens once, below,
        // against the *resolved* date via `iso::is_year_month_within_limits`,
        // not here.
        let requested_year =
            self.temporal_read_optional_integer(like, "year", i32::MIN, i32::MAX)?;

        let any_present = era_s.is_some()
            || requested_era_year.is_some()
            || requested_month.is_some()
            || month_code_s.is_some()
            || requested_year.is_some();
        if !any_present {
            return Err(RuntimeError::TypeError(
                "Temporal.with requires at least one recognized property".into(),
            ));
        }
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;

        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let month_code = month_code_s
            .clone()
            .or_else(|| requested_month.is_none().then(|| base.month_code.clone()));

        // `CalendarFields.cpp`'s `NonISOResolveFields`: on a calendar that
        // supports eras, `era` and `eraYear` must be supplied together or
        // not at all -- see
        // `development/browser_core/phase-26-ecma262-temporal/PLAN.md`'s
        // Stage 2 `plain_year_month.rs` entry and this test file's own
        // doc comment for the fixture this pins
        // (`mutually-exclusive-fields-gregory.js`).
        let supports_era = calendar::calendar_supports_era(&existing.calendar);
        if supports_era && era_s.is_some() != requested_era_year.is_some() {
            return Err(RuntimeError::TypeError(if era_s.is_some() {
                "Temporal.with requires eraYear when era is provided".into()
            } else {
                "Temporal.with requires era when eraYear is provided".into()
            }));
        }
        // `chinese`/`dangi` are unlike `iso8601` here even though both fail
        // `calendar_supports_era`: `iso8601` silently ignores an `era`/
        // `eraYear` property (no Test262 fixture requires otherwise, and
        // `PlainDate`'s own `with/time-units-ignored.js` establishes this is
        // the correct cross-type behavior for `iso8601` specifically), but
        // ICU4X has no era concept for `chinese`/`dangi` at all and
        // Temporal's own behavior for them is to *reject* any use of
        // `era`/`eraYear`, matching
        // `mutually-exclusive-fields-{chinese,dangi}.js`'s
        // `assert.throws(TypeError, () => instance.with({ eraYear, era }))`.
        if !supports_era
            && existing.calendar != "iso8601"
            && (era_s.is_some() || requested_era_year.is_some())
        {
            return Err(RuntimeError::TypeError(
                "era and eraYear are not valid for this calendar".into(),
            ));
        }
        // `NonISOFieldKeysToIgnore`: `era`/`eraYear`/`year` are mutually
        // exclusive as a group on an era-supporting calendar -- providing
        // any one of them drops the receiver's own value for all three,
        // rather than only the field actually given.
        let (era_field, era_year_field, extended_year_field) =
            if supports_era && era_s.is_some() && requested_era_year.is_some() {
                (era_s.as_deref(), requested_era_year, None)
            } else {
                (None, None, Some(requested_year.unwrap_or(base.year)))
            };
        let fields = plain_year_month::YearMonthFields {
            era: era_field,
            era_year: era_year_field,
            extended_year: extended_year_field,
            month_code: month_code.as_deref(),
            ordinal_month: requested_month.map(|value| value.min(i32::from(u8::MAX)) as u8),
        };
        let date = plain_year_month::year_month_from_fields(calendar_kind, &fields, reject)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar year-month".into()))?;
        if !iso::is_year_month_within_limits(date.0, date.1) {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainYearMonth is outside the supported range".into(),
            ));
        }
        let value =
            Self::temporal_date_value(TemporalKind::PlainYearMonth, existing.calendar, date);
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.PlainMonthDay.prototype.with`.
    pub(in super::super) fn temporal_month_day_with(
        &mut self,
        receiver: &Value,
        like: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_month_day_receiver(receiver)?;
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
        let base = self.temporal_calendar_fields(&existing)?;
        // `PreparePartialCalendarFields` reads and converts one field at a
        // time in alphabetical order (`day`, `month`, `monthCode`, `year`),
        // each conversion right after its own `Get` (`with/
        // order-of-operations.js`). That also keeps every object-valued
        // `Get` result from being held in a Rust local across a later call
        // that can allocate -- see `temporal_plain_month_day_from_fields`.
        //
        // `ToPositiveIntegerWithTruncation`: only a lower bound of `1`, no
        // upper bound at the field-reading stage -- `{ day: 100 }` must reach
        // the calendar's own `overflow: "constrain"`/`"reject"` regulation
        // below, not be rejected outright here. The `u8` field this feeds is
        // saturated rather than truncated so a huge value still clamps
        // sensibly.
        let requested_day = self.temporal_read_optional_integer(like, "day", 1, i32::MAX)?;
        let requested_month = self.temporal_read_optional_integer(like, "month", 1, i32::MAX)?;
        let month_code_s =
            self.temporal_read_optional_string(like, "monthCode", "invalid Temporal month code")?;
        // `ToIntegerWithTruncation`, unbounded at the field-reading stage,
        // matching `built-ins/Temporal/PlainMonthDay/prototype/with/
        // iso-year-used-only-for-overflow.js`: for `PlainMonthDay` a huge
        // out-of-range `year` is legitimate input used only to decide
        // leap-year-ness for `overflow` regulation, never range-checked
        // itself, and never part of the type's own identity.
        let requested_year =
            self.temporal_read_optional_integer(like, "year", i32::MIN, i32::MAX)?;
        if requested_year.is_none()
            && requested_month.is_none()
            && month_code_s.is_none()
            && requested_day.is_none()
        {
            return Err(RuntimeError::TypeError(
                "Temporal.with requires at least one recognized property".into(),
            ));
        }

        // `PrepareCalendarFields` (the field reads/coercions above, which
        // can themselves throw `RangeError` -- e.g. `{ day: -1 }`) runs
        // strictly before `GetOptionsObject`/`GetTemporalOverflowOption`,
        // not after -- pinned by `with/options-wrong-type.js`'s "Partial
        // date processed before throwing TypeError" case: an invalid field
        // must report `RangeError` even when `options` is itself a
        // wrong-type value that would otherwise throw `TypeError`.
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;

        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let month_code = month_code_s
            .clone()
            .or_else(|| requested_month.is_none().then(|| base.month_code.clone()));
        // `ISODateToFields(calendar, isoDate, MONTH-DAY)`: the receiver's
        // own base field set for `with()`'s merge is only `monthCode`/`day`
        // -- unlike `PlainDate`/`PlainYearMonth`, `PlainMonthDay` has no
        // `year`/`month` getters at all, so there is no receiver `year` to
        // fall back on for a non-ISO calendar. Falling back to `base.year`
        // regardless (as this function did before this fix) is only
        // correct for `iso8601`, whose fixed 1972 reference year is never
        // genuinely ambiguous; for any other calendar, supplying a bare
        // ordinal `month` with no explicit `year` must throw per
        // `NonISOResolveFields`'s `requireYear` rule -- pinned by
        // `intl402/Temporal/PlainMonthDay/prototype/with/
        // fields-missing-properties.js`.
        let extended_year_for_fields = if existing.calendar == "iso8601" {
            Some(requested_year.unwrap_or(base.year))
        } else {
            requested_year
        };
        if existing.calendar != "iso8601"
            && requested_month.is_some()
            && extended_year_for_fields.is_none()
        {
            return Err(RuntimeError::TypeError(
                "Temporal.with requires year when only an ordinal month is given for this calendar"
                    .into(),
            ));
        }
        // As in `temporal_plain_month_day_from_fields`: once a `monthCode`
        // is in hand, drop the redundant `ordinal_month` --
        // `icu_calendar::Date::try_from_fields` treats a simultaneous
        // `month_code` + `ordinal_month` as conflicting fields even when
        // they agree.
        let ordinal_month_for_fields = month_code
            .is_none()
            .then(|| requested_month.map(|value| value.min(i32::from(u8::MAX)) as u8))
            .flatten();
        let day_for_fields = requested_day
            .map(|value| value.min(i32::from(u8::MAX)) as u8)
            .unwrap_or(base.day);
        // As in `temporal_plain_month_day_from_fields`: the `iso8601`
        // calendar's own regulation must bypass `icu_calendar` entirely --
        // its internal year-range limits are far narrower than the
        // regulation year's legitimate domain (`year` here is only ever
        // used to decide leap-year-ness, never part of the result).
        // Pinned by `PlainMonthDay/prototype/with/
        // iso-year-used-only-for-overflow.js`.
        let date = if existing.calendar == "iso8601" {
            // A `month`/`monthCode` pair that disagree is always a
            // `RangeError` -- `with/basic.js`'s `{ month: 12, monthCode:
            // "M11" }` ("with({month, monthCode}) disagree"). Only
            // `month_code_s` (the value actually supplied to `with()`, not
            // `month_code`, which also carries the receiver's own
            // unmodified base value) participates in this check: a
            // `monthCode` the caller didn't touch must never conflict with
            // a newly supplied `month`.
            let month_code_ordinal = month_code_s
                .as_deref()
                .and_then(|code| code.strip_prefix('M')?.parse::<u8>().ok());
            if let (Some(month_num), Some(code_num)) = (requested_month, month_code_ordinal) {
                if month_num != i32::from(code_num) {
                    return Err(RuntimeError::RangeError(
                        "Temporal.PlainMonthDay month and monthCode conflict".into(),
                    ));
                }
            }
            let month = requested_month
                .map(|value| value.min(i32::from(u8::MAX)) as u8)
                .or_else(|| {
                    month_code
                        .as_deref()
                        .and_then(|code| code.strip_prefix('M')?.parse().ok())
                })
                .ok_or_else(|| RuntimeError::RangeError("invalid Temporal month code".into()))?;
            plain_month_day::iso_month_day_from_fields(
                month,
                day_for_fields,
                extended_year_for_fields.unwrap_or(1972),
                reject,
            )
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar month-day".into()))?
        } else {
            let fields = plain_month_day::MonthDayFields {
                extended_year: extended_year_for_fields,
                month_code: month_code.as_deref(),
                ordinal_month: ordinal_month_for_fields,
                day: day_for_fields,
                ..Default::default()
            };
            plain_month_day::month_day_from_fields(calendar_kind, &fields, reject).map_err(
                |_| RuntimeError::RangeError("invalid Temporal calendar month-day".into()),
            )?
        };
        let value = Self::temporal_date_value(TemporalKind::PlainMonthDay, existing.calendar, date);
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.PlainYearMonth.prototype.add`/`subtract`. Only a duration
    /// with zero weeks/days/time is accepted -- `AddDurationToYearMonth`'s
    /// own rule (Gecko's `NonZeroDurationPartAfterMonths`).
    pub(in super::super) fn temporal_year_month_add(
        &mut self,
        receiver: &Value,
        duration_value: &Value,
        options: &Value,
        negate: bool,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_year_month_receiver(receiver)?;
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
        // `AddDurationToYearMonth`: the options are read and cast first
        // (`options-read-before-algorithmic-validation.js`), then the day-1
        // date of the receiver is built -- which must be a valid `PlainDate`, so
        // the minimum year-month `-271821-04` (first day -271821-04-01) cannot be
        // added to even with a blank duration -- and only then is the duration
        // itself judged.
        let resolved_options = self.temporal_options(options)?;
        let reject = self.temporal_overflow_option(&resolved_options)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let fields = self.temporal_calendar_fields(&existing)?;
        let anchor_fields = plain_year_month::YearMonthFields {
            era: None,
            era_year: None,
            extended_year: Some(fields.year),
            month_code: Some(&fields.month_code),
            ordinal_month: None,
        };
        let anchor = plain_year_month::year_month_from_fields(calendar_kind, &anchor_fields, false)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar year-month".into()))?;
        if !epoch::is_date_within_limits(anchor) {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainYearMonth arithmetic is out of range".into(),
            ));
        }
        if duration.weeks != 0
            || duration.days != 0
            || duration.hours != 0
            || duration.minutes != 0
            || duration.seconds != 0
            || duration.milliseconds != 0
            || duration.microseconds != 0
            || duration.nanoseconds != 0
        {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainYearMonth arithmetic only accepts a years/months duration".into(),
            ));
        }
        let result_date = plain_date::calendar_add_date(
            calendar_kind,
            anchor,
            duration.years as i64,
            duration.months as i64,
            0,
            0,
            reject,
        )
        .ok_or_else(|| {
            RuntimeError::RangeError("Temporal.PlainYearMonth arithmetic is out of range".into())
        })?;
        if !iso::is_year_month_within_limits(result_date.0, result_date.1) {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainYearMonth arithmetic is out of range".into(),
            ));
        }
        let value =
            Self::temporal_date_value(TemporalKind::PlainYearMonth, existing.calendar, result_date);
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.PlainYearMonth.prototype.until`/`since`. `smallestUnit`/
    /// `largestUnit` are restricted to `"month"`/`"year"` -- `until`/`since`
    /// simply do not accept a finer unit for this type
    /// (`GetDifferenceSettings`'s own `disallowedUnits` for `YearMonth`).
    pub(in super::super) fn temporal_year_month_difference(
        &mut self,
        receiver: &Value,
        other_value: &Value,
        options: &Value,
        since: bool,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_year_month_receiver(receiver)?;
        let other = self.temporal_to_plain_year_month(other_value, &Value::Undefined)?;
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
            None => rounding::TemporalUnit::Month,
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid smallestUnit option".into()))?,
        };
        if !matches!(
            smallest_unit,
            rounding::TemporalUnit::Month | rounding::TemporalUnit::Year
        ) {
            return Err(RuntimeError::RangeError(
                "smallestUnit is out of range for this receiver".into(),
            ));
        }
        let largest_unit = match largest_raw.as_deref() {
            None | Some("auto") => rounding::TemporalUnit::Year,
            Some(text) => rounding::parse_temporal_unit(text)
                .ok_or_else(|| RuntimeError::RangeError("invalid largestUnit option".into()))?,
        };
        if !matches!(
            largest_unit,
            rounding::TemporalUnit::Month | rounding::TemporalUnit::Year
        ) {
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

        // Equal year-months are a blank duration before any date is built
        // (`DifferenceTemporalPlainYearMonth` step 7), so even the extreme
        // year-months whose first day is not a valid date can be compared with
        // themselves.
        if (existing.year, existing.month, existing.day) == (other.year, other.month, other.day) {
            return self.temporal_duration_create([0; 10]);
        }
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        // Always `from = existing, to = other` and negate the *result* for
        // `since`, never swap which date is `from`/`to` — matching
        // `temporal_date_difference`'s own documented rule (see that
        // function's own comment). `calendar_difference_date_leap_month`
        // anchors its whole computation on `from`'s own `Month` identity, so
        // swapping `from`/`to` instead of negating silently computes a
        // different (and for the three leap-month calendars, wrong)
        // quantity: `f(other, existing) != -f(existing, other)` in general.
        // Found via `intl402/Temporal/PlainYearMonth/prototype/since/
        // leap-months-{chinese,dangi,hebrew}.js`, whose "M04L-M04 is 1y not
        // 1y 1mo" case this swap computed as `1y 1mo` instead of `1y`.
        let (from, to) = (&existing, &other);
        let from_fields = self.temporal_calendar_fields(from)?;
        let to_fields = self.temporal_calendar_fields(to)?;
        let resolve = |fields: &TemporalCalendarFields| {
            plain_year_month::year_month_from_fields(
                calendar_kind,
                &plain_year_month::YearMonthFields {
                    era: None,
                    era_year: None,
                    extended_year: Some(fields.year),
                    month_code: Some(&fields.month_code),
                    ordinal_month: None,
                },
                false,
            )
        };
        let from_date = resolve(&from_fields)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar year-month".into()))?;
        let to_date = resolve(&to_fields)
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar year-month".into()))?;
        // Both first-of-month dates must be valid `PlainDate`s (steps 8-11), so
        // `-271821-04` and `+275760-10` are refused as arguments.
        if !epoch::is_date_within_limits(from_date) || !epoch::is_date_within_limits(to_date) {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainYearMonth difference is out of range".into(),
            ));
        }

        // `round_calendar_duration`'s own `roundingMode` is direction-
        // sensitive (`Ceil`/`Floor`/`HalfCeil`/`HalfFloor` round toward a
        // fixed end of the *real* number line, not toward a fixed end of
        // whichever internal `from`/`to` direction happened to be computed),
        // so negating the result below without also reflecting an
        // asymmetric mode would silently round the wrong way whenever
        // `since` negates — `ceil(-x) == -floor(x)`, not `-ceil(x)`. Found
        // via `built-ins/Temporal/PlainYearMonth/prototype/since/
        // roundingmode-{ceil,floor}.js`, which this exact reflection fixes.
        // `Trunc`/`Expand`/`HalfExpand`/`HalfTrunc`/`HalfEven` are all
        // symmetric under negation and need no reflection.
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
        // A `smallestUnit` of `month` with an increment of 1 needs no rounding at
        // all (step 16) -- and therefore builds no rounding window, whose far end
        // may lie outside the representable range at the very edge of it.
        let (years, months, _, _) =
            if smallest_unit == rounding::TemporalUnit::Month && increment == 1 {
                plain_date::calendar_difference_date(
                    calendar_kind,
                    from_date,
                    to_date,
                    Self::temporal_unit_to_date_unit(largest_unit),
                )
            } else {
                plain_date::round_calendar_duration(
                    calendar_kind,
                    from_date,
                    to_date,
                    Self::temporal_unit_to_date_unit(largest_unit),
                    Self::temporal_unit_to_date_unit(smallest_unit),
                    increment,
                    effective_mode,
                )
                .ok_or_else(|| {
                    RuntimeError::RangeError(
                        "Temporal.PlainYearMonth rounded date is out of range".into(),
                    )
                })?
            };
        let (years, months) = if since {
            (-years, -months)
        } else {
            (years, months)
        };
        let record = blueice_ecma402::DurationRecord::try_new(
            i128::from(years),
            i128::from(months),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        )
        .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
        self.alloc_temporal_value(Self::temporal_duration_value(record), false)
    }

    pub(in super::super) fn temporal_year_month_equals(
        &mut self,
        receiver: &Value,
        other_value: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_year_month_receiver(receiver)?;
        let other = self.temporal_to_plain_year_month(other_value, &Value::Undefined)?;
        let equal = existing.year == other.year
            && existing.month == other.month
            && existing.day == other.day
            && existing.calendar == other.calendar;
        Ok(Value::Bool(equal))
    }

    pub(in super::super) fn temporal_year_month_compare(
        &mut self,
        one: &Value,
        two: &Value,
    ) -> Result<Value, RuntimeError> {
        let a = self.temporal_to_plain_year_month(one, &Value::Undefined)?;
        let b = self.temporal_to_plain_year_month(two, &Value::Undefined)?;
        let ord = (a.year, a.month, a.day).cmp(&(b.year, b.month, b.day));
        Ok(Value::Number(match ord {
            std::cmp::Ordering::Less => -1.0,
            std::cmp::Ordering::Equal => 0.0,
            std::cmp::Ordering::Greater => 1.0,
        }))
    }

    pub(in super::super) fn temporal_month_day_equals(
        &mut self,
        receiver: &Value,
        other_value: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_month_day_receiver(receiver)?;
        let other = self.temporal_to_plain_month_day(other_value, &Value::Undefined)?;
        let equal = existing.year == other.year
            && existing.month == other.month
            && existing.day == other.day
            && existing.calendar == other.calendar;
        Ok(Value::Bool(equal))
    }

    pub(in super::super) fn temporal_year_month_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_year_month_receiver(receiver)?;
        let resolved_options = self.temporal_options(options)?;
        let show_calendar_raw = self.temporal_string_option(
            &resolved_options,
            "calendarName",
            &["auto", "always", "never", "critical"],
        )?;
        let show_calendar = show_calendar_raw
            .as_deref()
            .map(|value| plain_date::parse_show_calendar(value).expect("already validated"))
            .unwrap_or(plain_date::ShowCalendar::Auto);
        let text = plain_year_month::format_year_month(
            (existing.year, existing.month, existing.day),
            &existing.calendar,
            show_calendar,
        );
        Ok(Value::String(text.into()))
    }

    pub(in super::super) fn temporal_month_day_to_string(
        &mut self,
        receiver: &Value,
        options: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_month_day_receiver(receiver)?;
        let resolved_options = self.temporal_options(options)?;
        let show_calendar_raw = self.temporal_string_option(
            &resolved_options,
            "calendarName",
            &["auto", "always", "never", "critical"],
        )?;
        let show_calendar = show_calendar_raw
            .as_deref()
            .map(|value| plain_date::parse_show_calendar(value).expect("already validated"))
            .unwrap_or(plain_date::ShowCalendar::Auto);
        let text = plain_month_day::format_month_day(
            (existing.year, existing.month, existing.day),
            &existing.calendar,
            show_calendar,
        );
        Ok(Value::String(text.into()))
    }

    /// `CreateDateTimeFormat`'s `required` parameter here is DATE, the same
    /// as `PlainDate`'s own: a `timeStyle` option is rejected unconditionally,
    /// even alongside `dateStyle`
    /// (`intl402/.../PlainYearMonth/prototype/toLocaleString/
    /// datestyle-and-timestyle.js`) -- the mirror of
    /// `temporal_plain_time_to_locale_string`'s own `required = TIME` check.
    pub(in super::super) fn temporal_year_month_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.temporal_year_month_receiver(receiver)?;
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
            if self
                .date_time_format_data(&formatter)?
                .options()
                .time_style
                .is_some()
            {
                return Err(RuntimeError::TypeError(
                    "Temporal.PlainYearMonth.prototype.toLocaleString does not accept a timeStyle option"
                        .into(),
                ));
            }
            self.date_time_format_format(&formatter, receiver)
        })();
        self.stack.truncate(stack_base);
        result
    }

    /// `CreateDateTimeFormat`'s `required` parameter here is DATE, the same
    /// as `PlainDate`'s own -- see `temporal_year_month_to_locale_string`'s
    /// own doc comment
    /// (`intl402/.../PlainMonthDay/prototype/toLocaleString/
    /// datestyle-and-timestyle.js`).
    pub(in super::super) fn temporal_month_day_to_locale_string(
        &mut self,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        self.temporal_month_day_receiver(receiver)?;
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
            if self
                .date_time_format_data(&formatter)?
                .options()
                .time_style
                .is_some()
            {
                return Err(RuntimeError::TypeError(
                    "Temporal.PlainMonthDay.prototype.toLocaleString does not accept a timeStyle option"
                        .into(),
                ));
            }
            self.date_time_format_format(&formatter, receiver)
        })();
        self.stack.truncate(stack_base);
        result
    }

    pub(in super::super) fn temporal_year_month_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.PlainYearMonth cannot be converted to a primitive value".into(),
        ))
    }

    pub(in super::super) fn temporal_month_day_value_of(&mut self) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.PlainMonthDay cannot be converted to a primitive value".into(),
        ))
    }

    /// `Temporal.PlainYearMonth.prototype.toPlainDate`: merges the
    /// receiver's own year/monthCode with the required `item.day`.
    pub(in super::super) fn temporal_year_month_to_plain_date(
        &mut self,
        receiver: &Value,
        item: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_year_month_receiver(receiver)?;
        if item.object_id().is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainYearMonth.prototype.toPlainDate requires an object".into(),
            ));
        }
        let day_v = self.get_property(item, &"day".into())?;
        if matches!(day_v, Value::Undefined) {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainYearMonth.prototype.toPlainDate requires a day property".into(),
            ));
        }
        let day = self.temporal_integer(&day_v, 1, 31, "day")?;
        let base = self.temporal_calendar_fields(&existing)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let mut fields = DateFields::default();
        fields.extended_year = Some(base.year);
        fields.month_code = Some(base.month_code.as_bytes());
        fields.day = Some(day as u8);
        let mut icu_options = icu_calendar::options::DateFromFieldsOptions::default();
        icu_options.overflow = Some(icu_calendar::options::Overflow::Constrain);
        let date = Date::try_from_fields(fields, icu_options, AnyCalendar::new(calendar_kind))
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar date".into()))?;
        let value = Self::temporal_value_from_calendar_date(
            TemporalKind::PlainDate,
            existing.calendar,
            date,
        );
        // `-271821-04` can be a year-month, but its 18th is not a date
        // (`toPlainDate/limits.js`).
        if !epoch::is_date_within_limits((value.year, value.month, value.day)) {
            return Err(RuntimeError::RangeError(
                "Temporal.PlainDate is outside the supported range".into(),
            ));
        }
        self.alloc_temporal_value(value, false)
    }

    /// `Temporal.PlainMonthDay.prototype.toPlainDate`: merges the
    /// receiver's own monthCode/day with the required `item.year`.
    pub(in super::super) fn temporal_month_day_to_plain_date(
        &mut self,
        receiver: &Value,
        item: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_month_day_receiver(receiver)?;
        if item.object_id().is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay.prototype.toPlainDate requires an object".into(),
            ));
        }
        let year_v = self.get_property(item, &"year".into())?;
        // Per the actual spec text (`plainmonthday.html`,
        // `sec-temporal.plainmonthday.prototype.toplaindate`), step 6's
        // `PrepareCalendarFields(calendar, item, « year », « », « »)` has an
        // *empty* required-field list -- despite this function's own
        // now-outdated doc comment above, `year` was never literally
        // required here. `era`/`eraYear` (read as a `CalendarExtraFields`
        // side effect of requesting `year`, exactly as in
        // `temporal_plain_month_day_from_fields`) can resolve the year
        // instead, and Test262's own
        // `toPlainDate/infinity-throws-rangeerror.js` calls
        // `instance.toPlainDate({ era: "ad", eraYear: Infinity })` with no
        // `year` property at all, expecting `eraYear`'s own out-of-range
        // value to be what throws (`RangeError`), not a missing-`year`
        // `TypeError`.
        let requested_year = (!matches!(year_v, Value::Undefined))
            .then(|| {
                // Unbounded at the field-reading stage
                // (`ToIntegerWithTruncation`), matching every other Temporal
                // year field in this file -- the real representable-range
                // check happens afterward, once an actual date is resolved
                // (see the `iso8601` fast path below).
                // `toPlainDate/limits.js`'s own `-271821`/`275760` boundary
                // years are themselves in Temporal's representable range for
                // *some* month/day (just not every one), so they must reach
                // real date resolution rather than being rejected here.
                self.temporal_integer(&year_v, i32::MIN, i32::MAX, "year")
            })
            .transpose()?;
        let base = self.temporal_calendar_fields(&existing)?;
        let calendar_kind = calendar::calendar_kind(&existing.calendar)
            .expect("Temporal values retain a validated calendar identifier");
        let supports_era = calendar::calendar_supports_era(&existing.calendar);
        let (era_s, requested_era_year) = if supports_era {
            let era_v = self.get_property(item, &"era".into())?;
            let era_year_v = self.get_property(item, &"eraYear".into())?;
            let era_s = (!matches!(era_v, Value::Undefined))
                .then(|| self.coerce_string(&era_v))
                .transpose()?
                .map(|value| {
                    value
                        .to_utf8()
                        .map_err(|_| RuntimeError::RangeError("invalid Temporal era".into()))
                })
                .transpose()?;
            let requested_era_year = (!matches!(era_year_v, Value::Undefined))
                .then(|| self.temporal_integer(&era_year_v, i32::MIN, i32::MAX, "era year"))
                .transpose()?;
            if era_s.is_some() != requested_era_year.is_some() {
                return Err(RuntimeError::TypeError(
                    "Temporal era and eraYear must be supplied together".into(),
                ));
            }
            (era_s, requested_era_year)
        } else {
            (None, None)
        };
        if requested_year.is_none() && era_s.is_none() {
            return Err(RuntimeError::TypeError(
                "Temporal.PlainMonthDay.prototype.toPlainDate requires a year property".into(),
            ));
        }
        let value = if existing.calendar == "iso8601" {
            // `icu_calendar::Date::try_from_fields`'s own internal
            // `CONSTRUCTOR_YEAR_RANGE` (`-9999..=9999`) is far narrower than
            // Temporal's real range -- the same "Calendar year-range getter
            // bug" class this codebase's Stage 0 audit already fixed for
            // `temporal_calendar_fields`'s getters, reachable here too since
            // `toPlainDate({ year: -271821 })` is a boundary-valid year for
            // April 19th. `plain_date::regulate_iso_date` is pure Rust
            // arithmetic with no such limit; `overflow` is always
            // `"constrain"` here -- `toPlainDate` takes no options argument
            // at all to request `"reject"`.
            let month = base
                .month_code
                .strip_prefix('M')
                .and_then(|digits| digits.parse::<u8>().ok())
                .expect("a resolved PlainMonthDay's own monthCode is always well-formed");
            // `iso8601` never supports era (`calendar_supports_era`), so
            // `era_s` is always `None` here -- the check above guarantees
            // `requested_year` is `Some` whenever this branch is reached.
            let year = requested_year
                .expect("iso8601 has no era substitute, so year must be present here");
            let date = plain_date::regulate_iso_date(year, month, i64::from(base.day), false)
                .expect("regulate_iso_date only fails under overflow: reject");
            if !epoch::is_date_within_limits(date) {
                return Err(RuntimeError::RangeError(
                    "Temporal.PlainDate is outside the supported range".into(),
                ));
            }
            Self::temporal_date_value(TemporalKind::PlainDate, existing.calendar, date)
        } else {
            // As in `temporal_plain_month_day_from_fields`: `era`/`eraYear`
            // (supplied together) resolve the year entirely on their own via
            // `icu_calendar`'s own era-aware `Date::try_from_fields`,
            // mutually exclusive with a separately-supplied `year`.
            let mut fields = DateFields::default();
            fields.extended_year = era_s.is_none().then_some(requested_year).flatten();
            fields.era = era_s.as_deref().map(str::as_bytes);
            fields.era_year = requested_era_year;
            fields.month_code = Some(base.month_code.as_bytes());
            fields.day = Some(base.day);
            let mut icu_options = icu_calendar::options::DateFromFieldsOptions::default();
            icu_options.overflow = Some(icu_calendar::options::Overflow::Constrain);
            let date = Date::try_from_fields(fields, icu_options, AnyCalendar::new(calendar_kind))
                .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar date".into()))?;
            Self::temporal_value_from_calendar_date(
                TemporalKind::PlainDate,
                existing.calendar,
                date,
            )
        };
        self.alloc_temporal_value(value, false)
    }
}
