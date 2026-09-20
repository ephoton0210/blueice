// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `toString`, `toLocaleString` and `valueOf` for `PlainDate` and
//! `PlainDateTime`.

use super::super::*;

impl Vm {
    pub(in super::super::super) fn temporal_date_to_string(
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
    pub(in super::super::super) fn temporal_date_to_locale_string(
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

    pub(in super::super::super) fn temporal_date_value_of(
        &mut self,
    ) -> Result<Value, RuntimeError> {
        Err(RuntimeError::TypeError(
            "Temporal.PlainDate/PlainDateTime cannot be converted to a primitive value".into(),
        ))
    }
}
