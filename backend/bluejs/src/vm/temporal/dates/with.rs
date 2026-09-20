// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `Temporal.PlainDate.prototype.with` / `Temporal.PlainDateTime.prototype.with`.

use super::super::*;

impl Vm {
    /// `Temporal.PlainDate.prototype.with`/`Temporal.PlainDateTime.prototype.with`.
    /// A property bag only: a `calendar`/`timeZone` property, or a
    /// Temporal-like object, is a `TypeError`; at least one recognized
    /// calendar/time field must be present.
    pub(in super::super::super) fn temporal_date_with(
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
                // No range while reading: `RegulateTime` judges it once
                // `overflow` is known (constrain clamps, reject throws).
                (
                    self.temporal_read_optional_time_field(like, "hour")?,
                    self.temporal_read_optional_time_field(like, "microsecond")?,
                    self.temporal_read_optional_time_field(like, "millisecond")?,
                    self.temporal_read_optional_time_field(like, "minute")?,
                )
            } else {
                (None, None, None, None)
            };
        let requested_month = self.temporal_read_optional_integer(like, "month", 1, 99)?;
        let month_code_s = self.temporal_read_month_code(like)?;
        let (requested_nanosecond, requested_second) = if is_date_time {
            (
                self.temporal_read_optional_time_field(like, "nanosecond")?,
                self.temporal_read_optional_time_field(like, "second")?,
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
        fields.day = Some(
            requested_day
                .unwrap_or(i32::from(existing_fields.day))
                .min(i32::from(u8::MAX)) as u8,
        );

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
            // `RegulateTime`, once the date has resolved.
            let time = Self::temporal_regulate_time(
                [
                    requested_hour.unwrap_or(i64::from(existing.hour)),
                    requested_minute.unwrap_or(i64::from(existing.minute)),
                    requested_second.unwrap_or(i64::from(existing.second)),
                    requested_millisecond.unwrap_or(i64::from(existing.millisecond)),
                    requested_microsecond.unwrap_or(i64::from(existing.microsecond)),
                    requested_nanosecond.unwrap_or(i64::from(existing.nanosecond)),
                ],
                reject,
            )?;
            result.hour = time.0;
            result.minute = time.1;
            result.second = time.2;
            result.millisecond = time.3;
            result.microsecond = time.4;
            result.nanosecond = time.5;
        }
        self.alloc_temporal_value(result, false)
    }
}
