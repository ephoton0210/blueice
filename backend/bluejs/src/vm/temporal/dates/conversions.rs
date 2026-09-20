// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Converting a `PlainDate` to the other plain Temporal types.

use super::super::*;

impl Vm {
    pub(in super::super::super) fn temporal_plain_date_to_plain_date_time(
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
    pub(in super::super::super) fn temporal_plain_date_to_plain_year_month(
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
    pub(in super::super::super) fn temporal_plain_date_to_plain_month_day(
        &mut self,
        receiver: &Value,
    ) -> Result<Value, RuntimeError> {
        let existing = self.temporal_date_receiver(receiver)?;
        // `CalendarMonthDayFromFields` on the ISO calendar always uses the
        // reference ISO year 1972: the date's own year plays no part, since
        // `ISODateToFields(month-day)` carries just `monthCode` and `day`
        // (`toPlainMonthDay/basic.js`).
        if existing.calendar == "iso8601" {
            let date = plain_month_day::iso_month_day_from_fields(
                existing.month,
                existing.day,
                1972,
                false,
            )
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar month-day".into()))?;
            let value =
                Self::temporal_date_value(TemporalKind::PlainMonthDay, existing.calendar, date);
            return self.alloc_temporal_value(value, false);
        }
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
}
