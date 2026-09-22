// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `toZonedDateTime` on a plain date/date-time and `ZonedDateTime.prototype.toLocaleString`:
//! the two places a Temporal value crosses into time-zone and locale machinery.

use super::super::*;

impl Vm {
    pub(in super::super::super) fn temporal_plain_to_zoned_date_time(
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
    pub(in super::super::super) fn temporal_zoned_date_time_to_locale_string(
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
            let format = blueice_ecma402::DateTimeFormat::try_new(&locales, options)
                .map_err(|error| RuntimeError::RangeError(error.to_string()))?;
            // Like the plain types: the value's calendar must be the ISO one
            // or the formatter's own (`toLocaleString/calendar-mismatch.js`).
            Self::temporal_check_format_calendar(&format, &value)?;
            format
                .format(milliseconds)
                .map(|formatted| Value::String(formatted.into()))
                .map_err(|error| RuntimeError::RangeError(error.to_string()))
        })();
        self.stack.truncate(stack_base);
        result
    }
}
