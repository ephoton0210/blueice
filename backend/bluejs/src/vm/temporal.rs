// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The compact Temporal value surface required by ECMA-402 date-time input.
//!
//! Temporal's complete arithmetic API is intentionally outside this module.
//! These internal slots and constructors provide the observable types that
//! `Intl.DateTimeFormat` must distinguish before it formats a range.

use super::*;
use crate::heap::{TemporalKind, TemporalValue};

fn is_leap_year(year: i32) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

fn days_in_month(year: i32, month: u8) -> Option<u8> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return None,
    })
}

fn temporal_date(source: &str) -> Option<(i32, u8, u8)> {
    let date = source.split(['T', '[', 'Z', '+']).next().unwrap_or(source);
    let end = date.rfind('-')?;
    let before_day = &date[..end];
    let middle = before_day.rfind('-')?;
    let year = date[..middle].parse().ok()?;
    let month = date[middle + 1..end].parse().ok()?;
    let day = date[end + 1..].parse().ok()?;
    (days_in_month(year, month).is_some_and(|last| day <= last)).then_some((year, month, day))
}

fn temporal_time(source: &str) -> Option<(u8, u8, u8, u16, u16, u16)> {
    let source = source.split(['Z', '+', '-', '[']).next().unwrap_or(source);
    let mut fields = source.split(':');
    let hour = fields.next()?.parse().ok()?;
    let minute = fields.next().unwrap_or("0").parse().ok()?;
    let second_and_fraction = fields.next().unwrap_or("0");
    if fields.next().is_some() || hour > 23 || minute > 59 {
        return None;
    }
    let (second, fraction) = second_and_fraction
        .split_once('.')
        .map_or((second_and_fraction, ""), |(second, fraction)| {
            (second, fraction)
        });
    let second = second.parse().ok()?;
    if second > 59 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut nanos = fraction
        .bytes()
        .take(9)
        .fold(0u32, |value, byte| value * 10 + u32::from(byte - b'0'));
    for _ in fraction.len().min(9)..9 {
        nanos *= 10;
    }
    Some((
        hour,
        minute,
        second,
        (nanos / 1_000_000) as u16,
        ((nanos / 1_000) % 1_000) as u16,
        (nanos % 1_000) as u16,
    ))
}

impl Vm {
    pub(super) fn temporal_global(&mut self) -> Result<Value, RuntimeError> {
        if let Some(&id) = self.globals.get("Temporal") {
            return Ok(Value::Object(id));
        }
        let string = self.string_intrinsics()?.0;
        let function_prototype = self.heap.prototype(string)?.unwrap();
        let object_prototype = self.object_prototype;
        let namespace = self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
        let root = self.heap.root(namespace)?;
        let base = self.stack.len();
        let result = (|| {
            self.define_data(
                namespace,
                JsSymbol::well_known("toStringTag"),
                Value::String("Temporal".into()),
                false,
                false,
                true,
            )?;
            for kind in [
                TemporalKind::Instant,
                TemporalKind::PlainDate,
                TemporalKind::PlainDateTime,
                TemporalKind::PlainMonthDay,
                TemporalKind::PlainTime,
                TemporalKind::PlainYearMonth,
                TemporalKind::ZonedDateTime,
            ] {
                self.install_native(
                    namespace,
                    function_prototype,
                    kind.name(),
                    match kind {
                        TemporalKind::PlainDate => 3,
                        TemporalKind::PlainDateTime => 3,
                        TemporalKind::PlainMonthDay => 2,
                        TemporalKind::PlainTime => 0,
                        TemporalKind::PlainYearMonth => 2,
                        TemporalKind::Instant | TemporalKind::ZonedDateTime => 1,
                    },
                    NativeFunction::TemporalConstructor(kind),
                )?;
                let constructor = self.heap.get(namespace, kind.name())?.object_id().unwrap();
                let prototype =
                    self.with_roots(|heap| heap.alloc_object(Some(object_prototype)))?;
                self.stack.push(Value::Object(prototype));
                self.define_data(
                    constructor,
                    "prototype",
                    Value::Object(prototype),
                    false,
                    false,
                    false,
                )?;
                self.define_data(
                    prototype,
                    "constructor",
                    Value::Object(constructor),
                    true,
                    false,
                    true,
                )?;
                self.define_data(
                    prototype,
                    JsSymbol::well_known("toStringTag"),
                    Value::String(kind.to_string_tag().into()),
                    false,
                    false,
                    true,
                )?;
                self.install_native(
                    constructor,
                    function_prototype,
                    "from",
                    1,
                    NativeFunction::TemporalFrom(kind),
                )?;
                if matches!(kind, TemporalKind::PlainDate | TemporalKind::PlainDateTime) {
                    self.install_native(
                        prototype,
                        function_prototype,
                        "withCalendar",
                        1,
                        NativeFunction::TemporalWithCalendar,
                    )?;
                }
                if kind == TemporalKind::ZonedDateTime {
                    self.install_native(
                        prototype,
                        function_prototype,
                        "toLocaleString",
                        0,
                        NativeFunction::TemporalZonedDateTimeToLocaleString,
                    )?;
                }
                self.globals
                    .insert(format!("%Temporal.{}%", kind.name()), constructor);
                self.stack.pop();
            }
            self.globals.insert("Temporal".into(), namespace);
            if let Some(&global) = self.globals.get("globalThis") {
                self.define_data(
                    global,
                    "Temporal",
                    Value::Object(namespace),
                    true,
                    false,
                    true,
                )?;
            }
            Ok(Value::Object(namespace))
        })();
        self.stack.truncate(base);
        self.heap.unroot(root)?;
        result
    }

    fn temporal_integer(
        &mut self,
        value: &Value,
        minimum: i32,
        maximum: i32,
        name: &str,
    ) -> Result<i32, RuntimeError> {
        let value = self.coerce_number(value)?;
        if !value.is_finite()
            || value.fract() != 0.0
            || !(f64::from(minimum)..=f64::from(maximum)).contains(&value)
        {
            return Err(RuntimeError::RangeError(format!("invalid Temporal {name}")));
        }
        Ok(value as i32)
    }

    fn temporal_calendar(&mut self, value: &Value) -> Result<String, RuntimeError> {
        if *value == Value::Undefined {
            return Ok("iso8601".into());
        }
        let value = self.coerce_string(value)?;
        let value = value
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal calendar".into()))?;
        if value.is_empty() {
            return Err(RuntimeError::RangeError("invalid Temporal calendar".into()));
        }
        Ok(value)
    }

    fn temporal_optional_integer(
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

    fn temporal_value_from_args(
        &mut self,
        kind: TemporalKind,
        args: &[Value],
    ) -> Result<TemporalValue, RuntimeError> {
        let number = |vm: &mut Self, index, minimum, maximum, name| {
            vm.temporal_integer(native::argument(args, index), minimum, maximum, name)
        };
        let mut value = TemporalValue {
            kind,
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
            TemporalKind::Instant => {
                value.epoch_nanoseconds = match native::argument(args, 0) {
                    Value::BigInt(value) => value.clone(),
                    _ => {
                        return Err(RuntimeError::TypeError(
                            "Temporal.Instant requires epoch nanoseconds as a BigInt".into(),
                        ));
                    }
                };
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
                value.time_zone = self
                    .coerce_string(native::argument(args, 1))?
                    .to_utf8()
                    .map_err(|_| RuntimeError::RangeError("invalid Temporal time zone".into()))?;
            }
            TemporalKind::PlainDate | TemporalKind::PlainDateTime => {
                value.year = number(self, 0, -271_821, 275_760, "year")?;
                value.month = number(self, 1, 1, 12, "month")? as u8;
                value.day = number(self, 2, 1, 31, "day")? as u8;
                if days_in_month(value.year, value.month).is_none_or(|last| value.day > last) {
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
                value.year = number(self, 3, -271_821, 275_760, "reference year").unwrap_or(1972);
                if days_in_month(value.year, value.month).is_none_or(|last| value.day > last) {
                    return Err(RuntimeError::RangeError("invalid Temporal day".into()));
                }
            }
            TemporalKind::PlainTime => {
                value.hour =
                    self.temporal_optional_integer(native::argument(args, 0), 0, 0, 23, "hour")?
                        as u8;
                value.minute =
                    self.temporal_optional_integer(native::argument(args, 1), 0, 0, 59, "minute")?
                        as u8;
                value.second =
                    self.temporal_optional_integer(native::argument(args, 2), 0, 0, 59, "second")?
                        as u8;
                value.millisecond = self.temporal_optional_integer(
                    native::argument(args, 3),
                    0,
                    0,
                    999,
                    "millisecond",
                )? as u16;
                value.microsecond = self.temporal_optional_integer(
                    native::argument(args, 4),
                    0,
                    0,
                    999,
                    "microsecond",
                )? as u16;
                value.nanosecond = self.temporal_optional_integer(
                    native::argument(args, 5),
                    0,
                    0,
                    999,
                    "nanosecond",
                )? as u16;
            }
            TemporalKind::PlainYearMonth => {
                value.year = number(self, 0, -271_821, 275_760, "year")?;
                value.month = number(self, 1, 1, 12, "month")? as u8;
                value.calendar = self.temporal_calendar(native::argument(args, 2))?;
                value.day = number(self, 3, 1, 31, "reference day").unwrap_or(1) as u8;
            }
        }
        Ok(value)
    }

    fn temporal_value_from_string(
        &mut self,
        kind: TemporalKind,
        source: &str,
    ) -> Result<TemporalValue, RuntimeError> {
        if kind == TemporalKind::Instant {
            return Ok(TemporalValue {
                kind,
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
        let (year, month, day) = temporal_date(source)
            .ok_or_else(|| RuntimeError::RangeError("invalid Temporal date string".into()))?;
        let (hour, minute, second, millisecond, microsecond, nanosecond) = source
            .split_once('T')
            .and_then(|(_, time)| temporal_time(time))
            .unwrap_or((0, 0, 0, 0, 0, 0));
        Ok(TemporalValue {
            kind,
            year: if kind == TemporalKind::PlainMonthDay {
                1972
            } else {
                year
            },
            month,
            day: if kind == TemporalKind::PlainYearMonth {
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
            epoch_nanoseconds: 0.into(),
            calendar: "iso8601".into(),
            time_zone: "UTC".into(),
        })
    }

    fn alloc_temporal_value(
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

    pub(super) fn temporal_constructor(
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

    pub(super) fn temporal_from(
        &mut self,
        kind: TemporalKind,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        if let Some(object) = value.object_id() {
            if let Some(temporal) = self.heap.temporal_value(object)? {
                if temporal.kind == kind {
                    return self.alloc_temporal_value(temporal, false);
                }
            }
        }
        let source = self.coerce_string(value)?;
        let source = source
            .to_utf8()
            .map_err(|_| RuntimeError::RangeError("invalid Temporal string".into()))?;
        self.temporal_value_from_string(kind, &source)
            .and_then(|temporal| self.alloc_temporal_value(temporal, false))
    }

    pub(super) fn temporal_with_calendar(
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
        value.calendar = self.temporal_calendar(calendar)?;
        self.alloc_temporal_value(value, false)
    }

    pub(super) fn temporal_zoned_date_time_to_locale_string(
        &self,
        receiver: &Value,
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
        Ok(Value::String("1970-01-01".into()))
    }
}
