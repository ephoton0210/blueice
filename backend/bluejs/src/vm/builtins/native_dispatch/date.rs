// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use super::*;

fn days_from_civil(year: i128, month: i128, day: i128) -> i128 {
    // Howard Hinnant's proleptic-Gregorian civil-date conversion, with the
    // Unix epoch as day zero. Month is normalized by the caller to 1..=12.
    let year = year - i128::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let march_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * march_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

const MILLISECONDS_PER_DAY: f64 = 86_400_000.0;
const TIME_CLIP_LIMIT: f64 = 8_640_000_000_000_000.0;

#[derive(Clone, Copy)]
struct DateParts {
    year: i128,
    month: i128,
    day: i128,
    weekday: i128,
    hour: i128,
    minute: i128,
    second: i128,
    millisecond: i128,
}

fn time_clip(time: f64) -> f64 {
    if !time.is_finite() || time.abs() > TIME_CLIP_LIMIT {
        f64::NAN
    } else {
        // TimeClip explicitly canonicalizes both signed zeroes to +0.
        let clipped = time.trunc();
        if clipped == 0.0 {
            0.0
        } else {
            clipped
        }
    }
}

fn civil_from_days(days: i128) -> (i128, i128, i128) {
    // Inverse of `days_from_civil`, also from Howard Hinnant's public-domain
    // civil-calendar algorithms. ECMAScript Date uses the proleptic Gregorian
    // calendar, including the years before the Unix epoch.
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = month_from_march + if month_from_march < 10 { 3 } else { -9 };
    year += i128::from(month <= 2);
    (year, month, day)
}

fn date_parts(time: f64) -> Option<DateParts> {
    if !time.is_finite() {
        return None;
    }
    let days = (time / MILLISECONDS_PER_DAY).floor() as i128;
    let within_day = time - days as f64 * MILLISECONDS_PER_DAY;
    let within_day = within_day.round() as i128;
    let (year, month, day) = civil_from_days(days);
    Some(DateParts {
        year,
        month,
        day,
        weekday: (days + 4).rem_euclid(7),
        hour: within_day / 3_600_000,
        minute: within_day / 60_000 % 60,
        second: within_day / 1_000 % 60,
        millisecond: within_day % 1_000,
    })
}

fn date_year(year: i128) -> String {
    if (0..=9_999).contains(&year) {
        format!("{year:04}")
    } else if year >= 0 {
        format!("+{year:06}")
    } else {
        format!("-{:06}", -year)
    }
}

fn date_iso_string(time: f64) -> Option<JsString> {
    let parts = date_parts(time)?;
    Some(
        format!(
            "{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            date_year(parts.year),
            parts.month,
            parts.day,
            parts.hour,
            parts.minute,
            parts.second,
            parts.millisecond,
        )
        .into(),
    )
}

fn date_utc_string(time: f64) -> JsString {
    let Some(parts) = date_parts(time) else {
        return "Invalid Date".into();
    };
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        WEEKDAYS[parts.weekday as usize],
        parts.day,
        MONTHS[(parts.month - 1) as usize],
        date_display_year(parts.year),
        parts.hour,
        parts.minute,
        parts.second,
    )
    .into()
}

fn date_display_year(year: i128) -> String {
    if year >= 0 {
        format!("{year:04}")
    } else {
        format!("-{:04}", -year)
    }
}

fn date_date_string(time: f64) -> JsString {
    let Some(parts) = date_parts(time) else {
        return "Invalid Date".into();
    };
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {} {:02} {}",
        WEEKDAYS[parts.weekday as usize],
        MONTHS[(parts.month - 1) as usize],
        parts.day,
        date_display_year(parts.year),
    )
    .into()
}

fn date_time_string(time: f64) -> JsString {
    let Some(parts) = date_parts(time) else {
        return "Invalid Date".into();
    };
    format!(
        "{:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        parts.hour, parts.minute, parts.second,
    )
    .into()
}

fn date_string(time: f64) -> JsString {
    if !time.is_finite() {
        return "Invalid Date".into();
    }
    let date = date_date_string(time);
    let time = date_time_string(time);
    format!(
        "{} {}",
        date.to_utf8().expect("Date text is UTF-8"),
        time.to_utf8().expect("Date text is UTF-8")
    )
    .into()
}

fn parse_date_number(source: &str, width: usize) -> Option<(i128, &str)> {
    let prefix = source.get(..width)?;
    if !prefix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((prefix.parse().ok()?, &source[width..]))
}

fn parse_date_time(source: &str) -> Option<(i128, i128, i128)> {
    let mut parts = source.split(':');
    let hour = parts.next()?.parse().ok()?;
    let minute = parts.next()?.parse().ok()?;
    let second = parts.next()?.parse().ok()?;
    (parts.next().is_none() && hour <= 23 && minute <= 59 && second <= 59)
        .then_some((hour, minute, second))
}

fn parse_date_month(source: &str) -> Option<i128> {
    match source {
        "Jan" => Some(1),
        "Feb" => Some(2),
        "Mar" => Some(3),
        "Apr" => Some(4),
        "May" => Some(5),
        "Jun" => Some(6),
        "Jul" => Some(7),
        "Aug" => Some(8),
        "Sep" => Some(9),
        "Oct" => Some(10),
        "Nov" => Some(11),
        "Dec" => Some(12),
        _ => None,
    }
}

// ECMAScript only requires Date.parse to accept the standard ISO syntax, but
// it also requires a zero-millisecond Date's own toString and toUTCString
// output to round-trip. BlueJS' local timezone is UTC, so accept precisely
// the two strings emitted by the formatters above without widening this into
// an implementation-dependent legacy parser.
fn parse_own_date_string(source: &str) -> Option<f64> {
    let fields = source.split_ascii_whitespace().collect::<Vec<_>>();
    let (day, month, year, time) = match fields.as_slice() {
        [weekday, day, month, year, time, "GMT"] if weekday.ends_with(',') => {
            (*day, *month, *year, *time)
        }
        [_, month, day, year, time, "GMT+0000", "(Coordinated", "Universal", "Time)"] => {
            (*day, *month, *year, *time)
        }
        _ => return None,
    };
    let day = day.parse().ok()?;
    let month = parse_date_month(month)?;
    let year = year.parse().ok()?;
    let (hour, minute, second) = parse_date_time(time)?;
    let value = date_from_parts(year, month - 1, day, hour, minute, second, 0);
    let parts = date_parts(value)?;
    (parts.year == year && parts.month == month && parts.day == day).then_some(value)
}

fn parse_iso_date(source: &str) -> Option<f64> {
    // ISO 8601's canonical Date Time String Format is the interoperable Date
    // parse subset. Date-only input is UTC; a time without an offset uses the
    // host local zone, which BlueJS currently defines as UTC.
    let (year, rest, negative_extended_year) = if let Some(rest) = source.strip_prefix('+') {
        let (year, rest) = parse_date_number(rest, 6)?;
        (year, rest, false)
    } else if let Some(rest) = source.strip_prefix('-') {
        let (year, rest) = parse_date_number(rest, 6)?;
        (-year, rest, year == 0)
    } else {
        let (year, rest) = parse_date_number(source, 4)?;
        (year, rest, false)
    };
    if negative_extended_year {
        return None;
    }
    if rest.is_empty() && !source.starts_with(['+', '-']) {
        return Some(date_from_parts(year, 0, 1, 0, 0, 0, 0));
    }
    let rest = rest.strip_prefix('-')?;
    let (month, rest) = parse_date_number(rest, 2)?;
    let rest = rest.strip_prefix('-')?;
    let (day, rest) = parse_date_number(rest, 2)?;
    let mut hour = 0;
    let mut minute = 0;
    let mut second = 0;
    let mut millisecond = 0;
    let mut offset_minutes = 0;
    if !rest.is_empty() {
        let rest = rest.strip_prefix('T')?;
        let (parsed_hour, rest) = parse_date_number(rest, 2)?;
        hour = parsed_hour;
        let rest = rest.strip_prefix(':')?;
        let (parsed_minute, mut rest) = parse_date_number(rest, 2)?;
        minute = parsed_minute;
        if let Some(after_seconds) = rest.strip_prefix(':') {
            let (parsed_second, after_second) = parse_date_number(after_seconds, 2)?;
            second = parsed_second;
            rest = after_second;
            if let Some(after_fraction) = rest.strip_prefix('.') {
                let width = after_fraction
                    .bytes()
                    .take_while(u8::is_ascii_digit)
                    .count();
                if width == 0 {
                    return None;
                }
                let fraction = &after_fraction[..width];
                let mut milliseconds = fraction[..fraction.len().min(3)].parse::<i128>().ok()?;
                for _ in fraction.len().min(3)..3 {
                    milliseconds *= 10;
                }
                millisecond = milliseconds;
                rest = &after_fraction[width..];
            }
        }
        if rest == "Z" {
            // Already UTC.
        } else if let Some((sign, rest)) = rest
            .chars()
            .next()
            .filter(|sign| matches!(sign, '+' | '-'))
            .map(|sign| (sign, &rest[sign.len_utf8()..]))
        {
            let (offset_hour, rest) = parse_date_number(rest, 2)?;
            let rest = rest.strip_prefix(':')?;
            let (offset_minute, rest) = parse_date_number(rest, 2)?;
            if !rest.is_empty() || offset_hour > 23 || offset_minute > 59 {
                return None;
            }
            offset_minutes = (offset_hour * 60 + offset_minute) * if sign == '+' { 1 } else { -1 };
        } else if !rest.is_empty() {
            return None;
        }
    }
    if !(1..=12).contains(&month)
        || !(0..=24).contains(&hour)
        || minute > 59
        || second > 59
        || (hour == 24 && (minute != 0 || second != 0 || millisecond != 0))
    {
        return None;
    }
    let local_time = days_from_civil(year, month, day) as f64 * MILLISECONDS_PER_DAY
        + hour as f64 * 3_600_000.0
        + minute as f64 * 60_000.0
        + second as f64 * 1_000.0
        + millisecond as f64;
    let parts = date_parts(local_time)?;
    if parts.year != year || parts.month != month || parts.day != day {
        return None;
    }
    Some(time_clip(local_time - offset_minutes as f64 * 60_000.0))
}

fn date_from_parts(
    mut year: i128,
    month: i128,
    day: i128,
    hour: i128,
    minute: i128,
    second: i128,
    millisecond: i128,
) -> f64 {
    year += month.div_euclid(12);
    let month = month.rem_euclid(12) + 1;
    time_clip(
        days_from_civil(year, month, day) as f64 * MILLISECONDS_PER_DAY
            + hour as f64 * 3_600_000.0
            + minute as f64 * 60_000.0
            + second as f64 * 1_000.0
            + millisecond as f64,
    )
}

impl Vm {
    pub(in super::super::super) fn current_time() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as f64
    }

    pub(in super::super::super) fn date_receiver_time(
        &self,
        receiver: &Value,
    ) -> Result<f64, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Date method requires a Date receiver".into())
        })?;
        self.heap
            .date_value(object)
            .map_err(|_| RuntimeError::TypeError("Date method requires a Date receiver".into()))
    }

    pub(in super::super::super) fn date_constructor(
        &mut self,
        args: &[Value],
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if !construct {
            return Ok(Value::String(date_string(Self::current_time())));
        }
        let base = self.stack.len();
        self.stack.extend(args.iter().cloned());
        let result = (|| {
            let time = match args {
                [] => Self::current_time(),
                [value] => {
                    if let Some(object) = value.object_id() {
                        if let Ok(time) = self.heap.date_value(object) {
                            time
                        } else {
                            let primitive = self.coerce_primitive(value, "default")?;
                            if let Value::String(string) = primitive {
                                string
                                    .to_utf8()
                                    .ok()
                                    .and_then(|string| parse_iso_date(&string))
                                    .unwrap_or(f64::NAN)
                            } else {
                                time_clip(self.coerce_number(&primitive)?)
                            }
                        }
                    } else if let Value::String(string) = value {
                        string
                            .to_utf8()
                            .ok()
                            .and_then(|string| parse_iso_date(&string))
                            .unwrap_or(f64::NAN)
                    } else {
                        time_clip(self.coerce_number(value)?)
                    }
                }
                _ => match self.date_utc(args)? {
                    Value::Number(time) => time,
                    _ => unreachable!("Date.UTC returns a Number"),
                },
            };
            let default = self
                .date_prototype
                .expect("Date global initializes its prototype");
            let prototype = self.constructor_prototype(default)?;
            Ok(Value::Object(self.with_roots(|heap| {
                heap.alloc_date(time_clip(time), Some(prototype))
            })?))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super::super) fn date_parse(
        &mut self,
        value: &Value,
    ) -> Result<Value, RuntimeError> {
        let value = self.coerce_string(value)?;
        Ok(Value::Number(
            value
                .to_utf8()
                .ok()
                .and_then(|value| parse_iso_date(&value).or_else(|| parse_own_date_string(&value)))
                .unwrap_or(f64::NAN),
        ))
    }

    pub(in super::super::super) fn date_set(
        &mut self,
        setter: native::DateSetter,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Date method requires a Date receiver".into())
        })?;
        let prior = self.date_receiver_time(receiver)?;
        let base = self.stack.len();
        self.stack.push(receiver.clone());
        self.stack.extend(args.iter().cloned());
        let result = (|| {
            // Every supplied argument converts left-to-right before an
            // invalid Date value can return NaN. This ordering is observable
            // through user-defined valueOf methods.
            let numbers = args
                .iter()
                .map(|value| {
                    self.coerce_number(value).map(|number| {
                        (number.is_finite() && number.abs() <= i64::MAX as f64)
                            .then_some(number.trunc() as i128)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let first = numbers.first().copied().flatten();
            if numbers.iter().any(Option::is_none) {
                self.with_roots(|heap| heap.set_date_value(object, f64::NAN))?;
                return Ok(Value::Number(f64::NAN));
            }
            let parts = if prior.is_finite() {
                date_parts(prior).expect("finite Date time has calendar parts")
            } else if matches!(
                setter,
                native::DateSetter::FullYear | native::DateSetter::Year
            ) {
                date_parts(0.0).expect("the epoch has calendar parts")
            } else {
                // The conversion above has already happened for an invalid
                // receiver, but its [[DateValue]] remains NaN.
                return Ok(Value::Number(f64::NAN));
            };
            let Some(first) = first else {
                self.with_roots(|heap| heap.set_date_value(object, f64::NAN))?;
                return Ok(Value::Number(f64::NAN));
            };
            let optional = |index: usize, default: i128| {
                numbers.get(index).copied().flatten().unwrap_or(default)
            };
            let result = match setter {
                native::DateSetter::Date => date_from_parts(
                    parts.year,
                    parts.month - 1,
                    first,
                    parts.hour,
                    parts.minute,
                    parts.second,
                    parts.millisecond,
                ),
                native::DateSetter::FullYear | native::DateSetter::Year => {
                    let year = if matches!(setter, native::DateSetter::Year)
                        && (0..=99).contains(&first)
                    {
                        first + 1900
                    } else {
                        first
                    };
                    let month = optional(1, parts.month - 1);
                    let day = optional(2, parts.day);
                    date_from_parts(
                        year,
                        month,
                        day,
                        parts.hour,
                        parts.minute,
                        parts.second,
                        parts.millisecond,
                    )
                }
                native::DateSetter::Hours => {
                    let minute = optional(1, parts.minute);
                    let second = optional(2, parts.second);
                    let millisecond = optional(3, parts.millisecond);
                    date_from_parts(
                        parts.year,
                        parts.month - 1,
                        parts.day,
                        first,
                        minute,
                        second,
                        millisecond,
                    )
                }
                native::DateSetter::Milliseconds => date_from_parts(
                    parts.year,
                    parts.month - 1,
                    parts.day,
                    parts.hour,
                    parts.minute,
                    parts.second,
                    first,
                ),
                native::DateSetter::Minutes => {
                    let second = optional(1, parts.second);
                    let millisecond = optional(2, parts.millisecond);
                    date_from_parts(
                        parts.year,
                        parts.month - 1,
                        parts.day,
                        parts.hour,
                        first,
                        second,
                        millisecond,
                    )
                }
                native::DateSetter::Month => {
                    let day = optional(1, parts.day);
                    date_from_parts(
                        parts.year,
                        first,
                        day,
                        parts.hour,
                        parts.minute,
                        parts.second,
                        parts.millisecond,
                    )
                }
                native::DateSetter::Seconds => {
                    let millisecond = optional(1, parts.millisecond);
                    date_from_parts(
                        parts.year,
                        parts.month - 1,
                        parts.day,
                        parts.hour,
                        parts.minute,
                        first,
                        millisecond,
                    )
                }
            };
            self.with_roots(|heap| heap.set_date_value(object, result))?;
            Ok(Value::Number(result))
        })();
        self.stack.truncate(base);
        result
    }

    pub(in super::super::super) fn date_method(
        &mut self,
        method: native::DateMethod,
        receiver: &Value,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let value = native::argument(args, 0);
        match method {
            native::DateMethod::Get(part) => {
                let time = self.date_receiver_time(receiver)?;
                let Some(parts) = date_parts(time) else {
                    return Ok(Value::Number(f64::NAN));
                };
                Ok(Value::Number(match part {
                    native::DatePart::Date => parts.day,
                    native::DatePart::Day => parts.weekday,
                    native::DatePart::FullYear => parts.year,
                    native::DatePart::Hours => parts.hour,
                    native::DatePart::Milliseconds => parts.millisecond,
                    native::DatePart::Minutes => parts.minute,
                    native::DatePart::Month => parts.month - 1,
                    native::DatePart::Seconds => parts.second,
                    // The current BlueJS host-local time zone is UTC.
                    native::DatePart::TimezoneOffset => 0,
                } as f64))
            }
            native::DateMethod::GetYear => {
                let time = self.date_receiver_time(receiver)?;
                Ok(date_parts(time)
                    .map(|parts| Value::Number((parts.year - 1900) as f64))
                    .unwrap_or(Value::Number(f64::NAN)))
            }
            native::DateMethod::GetTime | native::DateMethod::ValueOf => {
                Ok(Value::Number(self.date_receiver_time(receiver)?))
            }
            native::DateMethod::Set(setter) => self.date_set(setter, receiver, args),
            native::DateMethod::SetTime => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError("Date method requires a Date receiver".into())
                })?;
                // Validate the receiver before observable number conversion.
                self.date_receiver_time(receiver)?;
                let time = time_clip(self.coerce_number(value)?);
                self.with_roots(|heap| heap.set_date_value(object, time))?;
                Ok(Value::Number(time))
            }
            native::DateMethod::ToIsoString => date_iso_string(self.date_receiver_time(receiver)?)
                .map(Value::String)
                .ok_or_else(|| RuntimeError::RangeError("invalid time value".into())),
            native::DateMethod::ToDateString => Ok(Value::String(date_date_string(
                self.date_receiver_time(receiver)?,
            ))),
            native::DateMethod::ToString => Ok(Value::String(date_string(
                self.date_receiver_time(receiver)?,
            ))),
            native::DateMethod::ToTimeString => Ok(Value::String(date_time_string(
                self.date_receiver_time(receiver)?,
            ))),
            native::DateMethod::ToTemporalInstant => {
                let time = self.date_receiver_time(receiver)?;
                if time.is_nan() {
                    return Err(RuntimeError::RangeError(
                        "an invalid Date has no Temporal.Instant".into(),
                    ));
                }
                // `TimeClip` already made a valid time value an integer within
                // +/-8.64e15, so `NumberToBigInt` cannot fail and the product
                // is always inside Temporal's instant range.
                self.instant_from_epoch_nanoseconds(BigInt::from(time as i64) * 1_000_000_u32)
            }
            native::DateMethod::ToUtcString => Ok(Value::String(date_utc_string(
                self.date_receiver_time(receiver)?,
            ))),
            native::DateMethod::ToPrimitive => {
                let Value::String(hint) = value else {
                    return Err(RuntimeError::TypeError(
                        "Date @@toPrimitive hint must be a string".into(),
                    ));
                };
                let names = match hint.to_utf8().as_deref() {
                    Ok("string") | Ok("default") => ["toString", "valueOf"],
                    Ok("number") => ["valueOf", "toString"],
                    _ => {
                        return Err(RuntimeError::TypeError(
                            "Date @@toPrimitive hint is invalid".into(),
                        ))
                    }
                };
                if receiver.object_id().is_none() {
                    return Err(RuntimeError::TypeError(
                        "Date @@toPrimitive requires an object receiver".into(),
                    ));
                }
                for name in names {
                    let method = self.get_property(receiver, &name.into())?;
                    if !self.is_callable(&method)? {
                        continue;
                    }
                    let result = self.call_native(method, receiver.clone(), Vec::new(), false)?;
                    if !matches!(result, Value::Object(_)) {
                        return Ok(result);
                    }
                }
                Err(RuntimeError::TypeError(
                    "Date primitive conversion did not produce a primitive".into(),
                ))
            }
            native::DateMethod::ToLocaleDateString => {
                self.date_to_locale_string(self.date_receiver_time(receiver)?, args, true, false)
            }
            native::DateMethod::ToLocaleString => {
                self.date_to_locale_string(self.date_receiver_time(receiver)?, args, true, true)
            }
            native::DateMethod::ToLocaleTimeString => {
                self.date_to_locale_string(self.date_receiver_time(receiver)?, args, false, true)
            }
            native::DateMethod::ToJson => {
                self.coerce_object(receiver)?;
                let primitive = self.coerce_primitive(receiver, "number")?;
                if let Value::Number(number) = primitive {
                    if !number.is_finite() {
                        return Ok(Value::Null);
                    }
                }
                let to_iso_string = self.get_property(receiver, &"toISOString".into())?;
                if !self.is_callable(&to_iso_string)? {
                    return Err(RuntimeError::TypeError(
                        "Date toISOString must be callable".into(),
                    ));
                }
                self.call_native(to_iso_string, receiver.clone(), Vec::new(), false)
            }
        }
    }

    pub(in super::super::super) fn date_utc(
        &mut self,
        args: &[Value],
    ) -> Result<Value, RuntimeError> {
        let defaults = [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let mut components = [0.0; 7];
        let mut invalid = false;
        for (index, component) in components.iter_mut().enumerate() {
            let number = if index < args.len() {
                self.coerce_number(&args[index])?
            } else if index == 0 {
                f64::NAN
            } else {
                defaults[index]
            };
            if !number.is_finite() {
                invalid = true;
            } else {
                *component = number.trunc();
            }
        }
        if invalid {
            return Ok(Value::Number(f64::NAN));
        }
        let mut year = components[0];
        if (0.0..=99.0).contains(&year) {
            year += 1900.0;
        }
        if year.abs() > i64::MAX as f64
            || components[1].abs() > i64::MAX as f64
            || components[2].abs() > i64::MAX as f64
        {
            return Ok(Value::Number(f64::NAN));
        }
        let mut year = year as i128;
        let month = components[1] as i128;
        year += month.div_euclid(12);
        let month = month.rem_euclid(12) + 1;
        // MakeTime and MakeDate deliberately perform each multiplication and
        // addition as IEEE-754 Number arithmetic. Collapsing this to an exact
        // integer calculation changes the observable rounding/cancellation
        // behavior for large, but still TimeClip-valid, components.
        let time = (components[3] * 3_600_000.0 + components[4] * 60_000.0)
            + components[5] * 1_000.0
            + components[6];
        let milliseconds =
            days_from_civil(year, month, components[2] as i128) as f64 * 86_400_000.0 + time;
        if !milliseconds.is_finite() || milliseconds.abs() > 8_640_000_000_000_000.0 {
            Ok(Value::Number(f64::NAN))
        } else {
            Ok(Value::Number(milliseconds.trunc()))
        }
    }
}
