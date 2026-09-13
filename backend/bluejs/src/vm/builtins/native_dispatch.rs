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
    fn current_time() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as f64
    }

    fn date_receiver_time(&self, receiver: &Value) -> Result<f64, RuntimeError> {
        let object = receiver.object_id().ok_or_else(|| {
            RuntimeError::TypeError("Date method requires a Date receiver".into())
        })?;
        self.heap
            .date_value(object)
            .map_err(|_| RuntimeError::TypeError("Date method requires a Date receiver".into()))
    }

    fn date_constructor(&mut self, args: &[Value], construct: bool) -> Result<Value, RuntimeError> {
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

    fn date_parse(&mut self, value: &Value) -> Result<Value, RuntimeError> {
        let value = self.coerce_string(value)?;
        Ok(Value::Number(
            value
                .to_utf8()
                .ok()
                .and_then(|value| parse_iso_date(&value).or_else(|| parse_own_date_string(&value)))
                .unwrap_or(f64::NAN),
        ))
    }

    fn date_set(
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

    fn date_method(
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
            native::DateMethod::ToLocaleDateString => Ok(Value::String(date_date_string(
                self.date_receiver_time(receiver)?,
            ))),
            native::DateMethod::ToLocaleString => Ok(Value::String(date_string(
                self.date_receiver_time(receiver)?,
            ))),
            native::DateMethod::ToLocaleTimeString => Ok(Value::String(date_time_string(
                self.date_receiver_time(receiver)?,
            ))),
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

    fn date_utc(&mut self, args: &[Value]) -> Result<Value, RuntimeError> {
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

    pub(in super::super) fn native_call(
        &mut self,
        function: NativeFunction,
        receiver: Value,
        args: Vec<Value>,
        construct: bool,
    ) -> Result<Value, RuntimeError> {
        if receiver
            .object_id()
            .is_some_and(|id| self.test262_foreign_reference(id).is_some())
            && matches!(
                function,
                NativeFunction::ArrayIteratorNext
                    | NativeFunction::IteratorNext
                    | NativeFunction::RegExpIteratorNext
            )
        {
            return self.test262_foreign_next(&receiver, &args);
        }
        let first = native::argument(&args, 0);
        match function {
            NativeFunction::Promise => self.promise_constructor(first.clone(), construct),
            NativeFunction::PromiseResolvingFunction { promise, fulfill } => {
                if fulfill {
                    self.resolve_promise(promise, first.clone())?;
                } else {
                    self.settle_promise(promise, PromiseStatus::Rejected(first.clone()))?;
                }
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseCapabilityExecutor { storage } => {
                let resolve = native::argument(&args, 0).clone();
                let reject = native::argument(&args, 1).clone();
                self.with_roots(|heap| heap.set(storage, "resolve", resolve))?;
                self.with_roots(|heap| heap.set(storage, "reject", reject))?;
                Ok(Value::Undefined)
            }
            NativeFunction::AsyncFromSyncFulfill { target, done } => {
                let result = self.iterator_result(first.clone(), done);
                match result {
                    Ok(result) => self.settle_promise(target, PromiseStatus::Fulfilled(result))?,
                    Err(error) => {
                        let error = self.error_value(error)?;
                        self.settle_promise(target, PromiseStatus::Rejected(error))?;
                    }
                }
                Ok(Value::Undefined)
            }
            NativeFunction::AsyncFromSyncReject { target, record } => {
                // AsyncFromSyncIteratorContinuation closes with an existing
                // throw completion. IteratorClose must retain that original
                // rejection even when the delegate's return method fails or
                // returns a non-object.
                let _ = self.iterator_close(&Value::Object(record));
                self.settle_promise(target, PromiseStatus::Rejected(first.clone()))?;
                Ok(Value::Undefined)
            }
            NativeFunction::AbstractModuleSource => Err(RuntimeError::TypeError(
                "AbstractModuleSource is an abstract constructor".into(),
            )),
            NativeFunction::AbstractModuleSourceToStringTag => Ok(Value::Undefined),
            NativeFunction::Function => self.function_constructor(&args),
            NativeFunction::AsyncFunction => self.async_function_constructor(&args),
            NativeFunction::Error(name) => self.error_constructor(name, &args, construct),
            NativeFunction::ErrorToString => self.error_to_string(&receiver),
            NativeFunction::Test262(name) => self.test262_call(name, &args),
            NativeFunction::Test262Done => {
                self.test262_done = Some(if matches!(first, Value::Undefined) {
                    Ok(())
                } else {
                    Err(first.clone())
                });
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseThen => self.promise_then(&receiver, &args),
            NativeFunction::PromiseCatch => self.promise_catch(&receiver, first),
            NativeFunction::PromiseFinally => self.promise_finally(&receiver, first),
            NativeFunction::PromiseResolve => {
                self.promise_resolve_constructor(&receiver, first.clone())
            }
            NativeFunction::PromiseReject => self.promise_reject(first.clone()),
            NativeFunction::PromiseAll => self.promise_all(&receiver, first),
            NativeFunction::PromiseAllResolve { target, index } => {
                self.promise_all_settled(target, index, first.clone())?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseAllReject { target } => {
                self.promise_all_reject(target, first.clone())?;
                Ok(Value::Undefined)
            }
            NativeFunction::PromiseWithResolvers => self.promise_with_resolvers(),
            NativeFunction::ToLocaleLowerCase
            | NativeFunction::ToLocaleUpperCase
            | NativeFunction::LocaleCompare => {
                let string = self.string_receiver(&receiver)?;
                if function == NativeFunction::LocaleCompare {
                    let other = self.coerce_string(first)?;
                    let collator = self
                        .resolve_collator(native::argument(&args, 1), native::argument(&args, 2))?;
                    Ok(collator.compare(&string, &other))
                } else {
                    let locales = self.canonical_locales(first)?;
                    let locale = locales
                        .first()
                        .cloned()
                        .unwrap_or(icu_locale_core::locale!("en-US"));
                    crate::intl::case_map(
                        &string,
                        &locale,
                        function == NativeFunction::ToLocaleUpperCase,
                        self.config.max_string_bytes,
                    )
                    .map(Value::String)
                }
            }
            NativeFunction::Collator => self.create_collator(&args, construct),
            NativeFunction::Locale => self.create_locale(&args, construct),
            NativeFunction::CanonicalLocales => {
                let locales = self.canonical_locales(first)?;
                self.array_from(
                    locales
                        .into_iter()
                        .map(|l| Value::String(l.to_string().into()))
                        .collect(),
                )
            }
            NativeFunction::SupportedLocales => self.supported_locales(&args),
            NativeFunction::CollatorCompareGetter => self.collator_compare_getter(&receiver),
            NativeFunction::CollatorCompare => {
                let collator = self.collator_data(&receiver)?;
                let left = self.coerce_string(first)?;
                let right = self.coerce_string(native::argument(&args, 1))?;
                Ok(collator.compare(&left, &right))
            }
            NativeFunction::CollatorResolvedOptions => self.collator_resolved_options(&receiver),
            NativeFunction::LocaleToString => self.locale_to_string(&receiver),
            NativeFunction::LocaleMaximize => self.locale_transform(&receiver, true),
            NativeFunction::LocaleMinimize => self.locale_transform(&receiver, false),
            NativeFunction::LocaleGetter(name) => self.locale_getter(&receiver, name),
            NativeFunction::LocaleInfo(name) => self.locale_info(&receiver, name),
            NativeFunction::Array => {
                let prototype = if construct {
                    self.constructor_prototype(self.array_prototype)?
                } else {
                    self.array_prototype
                };
                if args.len() == 1 {
                    if let Value::Number(length) = first {
                        let Value::Number(length) =
                            self.array_length_value(&Value::Number(*length))?
                        else {
                            unreachable!()
                        };
                        return Ok(Value::Object(self.with_roots(|heap| {
                            heap.alloc_array(length as u32, Some(prototype))
                        })?));
                    }
                }
                self.array_from_with_prototype(args, prototype)
            }
            NativeFunction::Date => self.date_constructor(&args, construct),
            NativeFunction::DateNow => Ok(Value::Number(Self::current_time())),
            NativeFunction::DateParse => self.date_parse(first),
            NativeFunction::DateUtc => self.date_utc(&args),
            NativeFunction::DateMethod(method) => self.date_method(method, &receiver, &args),
            NativeFunction::ArrayBuffer => self.array_buffer_constructor(&args, construct),
            NativeFunction::ArrayBufferByteLength => Ok(Value::Number(
                self.heap
                    .array_buffer_byte_length(self.array_buffer_receiver(&receiver)?)?
                    as f64,
            )),
            NativeFunction::ArrayBufferMaxByteLength => Ok(Value::Number(
                self.heap
                    .buffer_max_byte_length(self.array_buffer_receiver(&receiver)?)?
                    as f64,
            )),
            NativeFunction::ArrayBufferResizable => Ok(Value::Bool(
                self.heap
                    .buffer_resizable(self.array_buffer_receiver(&receiver)?)?,
            )),
            NativeFunction::ArrayBufferResize => self.buffer_resize(&receiver, first),
            NativeFunction::ArrayBufferTransfer => {
                self.array_buffer_transfer(&receiver, &args, false)
            }
            NativeFunction::ArrayBufferTransferToFixedLength => {
                self.array_buffer_transfer(&receiver, &args, true)
            }
            NativeFunction::ArrayBufferSlice => self.array_buffer_slice(&receiver, &args),
            NativeFunction::ArrayBufferIsView => {
                Ok(Value::Bool(first.object_id().is_some_and(|object| {
                    self.heap.is_data_view(object).unwrap_or(false)
                        || self.heap.is_typed_array(object).unwrap_or(false)
                })))
            }
            NativeFunction::ArrayBufferSpecies => Ok(receiver),
            NativeFunction::SharedArrayBuffer => {
                self.shared_array_buffer_constructor(&args, construct)
            }
            NativeFunction::SharedArrayBufferByteLength => Ok(Value::Number(
                self.heap
                    .buffer_byte_length(self.shared_array_buffer_receiver(&receiver)?)?
                    as f64,
            )),
            NativeFunction::SharedArrayBufferMaxByteLength => Ok(Value::Number(
                self.heap
                    .buffer_max_byte_length(self.shared_array_buffer_receiver(&receiver)?)?
                    as f64,
            )),
            NativeFunction::SharedArrayBufferGrowable => Ok(Value::Bool(
                self.heap
                    .buffer_growable(self.shared_array_buffer_receiver(&receiver)?)?,
            )),
            NativeFunction::SharedArrayBufferGrow => self.shared_buffer_grow(&receiver, first),
            NativeFunction::SharedArrayBufferSlice => {
                self.shared_array_buffer_slice(&receiver, &args)
            }
            NativeFunction::SharedArrayBufferSpecies => Ok(receiver),
            NativeFunction::Atomics(operation) => self.atomics_operation(&args, operation),
            NativeFunction::AtomicsIsLockFree => self.atomics_is_lock_free(first),
            NativeFunction::AtomicsNotify => self.atomics_notify(&args),
            NativeFunction::AtomicsPause => Ok(Value::Undefined),
            NativeFunction::AtomicsWait => self.atomics_wait(&args),
            NativeFunction::AtomicsWaitAsync => self.atomics_wait_async(&args),
            NativeFunction::DataView => self.data_view_constructor(&args, construct),
            NativeFunction::DataViewBuffer => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError("DataView method requires a DataView receiver".into())
                })?;
                let buffer = self
                    .heap
                    .data_view_buffer(object)
                    .map_err(|error| match error {
                        HeapError::InvalidInternalSlot(_) | HeapError::InvalidObject(_) => {
                            RuntimeError::TypeError(
                                "DataView method requires a DataView receiver".into(),
                            )
                        }
                        error => error.into(),
                    })?;
                Ok(Value::Object(buffer))
            }
            NativeFunction::DataViewByteLength => {
                let (_, _, length) = self.data_view_receiver(&receiver)?;
                Ok(Value::Number(length as f64))
            }
            NativeFunction::DataViewByteOffset => {
                let (_, offset, _) = self.data_view_receiver(&receiver)?;
                Ok(Value::Number(offset as f64))
            }
            NativeFunction::DataViewGet {
                width,
                signed,
                floating,
                bigint,
            } => self.data_view_get(&receiver, &args, width, signed, floating, bigint),
            NativeFunction::DataViewSet {
                width,
                signed,
                floating,
                bigint,
            } => self.data_view_set(&receiver, &args, width, signed, floating, bigint),
            NativeFunction::TypedArray(kind) => {
                self.typed_array_constructor(&args, construct, kind)
            }
            NativeFunction::TypedArrayIntrinsic => Err(RuntimeError::TypeError(
                "%TypedArray% is not directly constructible".into(),
            )),
            NativeFunction::TypedArrayBuffer => {
                let (buffer, _, _, _) = self.typed_array_receiver(&receiver)?;
                Ok(Value::Object(buffer))
            }
            NativeFunction::TypedArrayByteLength => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError(
                        "TypedArray method requires a TypedArray receiver".into(),
                    )
                })?;
                let (buffer, _, length, kind) = self.heap.typed_array_info(object)?;
                let byte_length = if self.heap.buffer_is_detached(buffer)?
                    || self.heap.typed_array_is_out_of_bounds(object)?
                {
                    0
                } else {
                    length * kind.byte_width()
                };
                Ok(Value::Number(byte_length as f64))
            }
            NativeFunction::TypedArrayByteOffset => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError(
                        "TypedArray method requires a TypedArray receiver".into(),
                    )
                })?;
                let (buffer, offset, _, _) = self.heap.typed_array_info(object)?;
                let byte_offset = if self.heap.buffer_is_detached(buffer)?
                    || self.heap.typed_array_is_out_of_bounds(object)?
                {
                    0
                } else {
                    offset
                };
                Ok(Value::Number(byte_offset as f64))
            }
            NativeFunction::TypedArrayLength => {
                let object = receiver.object_id().ok_or_else(|| {
                    RuntimeError::TypeError(
                        "TypedArray method requires a TypedArray receiver".into(),
                    )
                })?;
                let (buffer, _, length, _) = self.heap.typed_array_info(object)?;
                let element_length = if self.heap.buffer_is_detached(buffer)?
                    || self.heap.typed_array_is_out_of_bounds(object)?
                {
                    0
                } else {
                    length
                };
                Ok(Value::Number(element_length as f64))
            }
            NativeFunction::TypedArraySet => self.typed_array_set(&receiver, &args),
            NativeFunction::TypedArraySubarray => self.typed_array_subarray(&receiver, &args),
            NativeFunction::TypedArraySpecies => Ok(receiver),
            NativeFunction::TypedArrayIterator(kind) => {
                self.typed_array_receiver(&receiver)?;
                let object = receiver
                    .object_id()
                    .expect("validated TypedArray receiver has an object identity");
                let prototype = self.array_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_array_iterator(object, kind, prototype)
                })?))
            }
            NativeFunction::TypedArrayMethod(method) => {
                self.typed_array_method(&receiver, &args, method)
            }
            NativeFunction::Proxy => self.proxy_constructor(&args, construct),
            NativeFunction::ProxyRevocable => self.proxy_revocable(&args),
            NativeFunction::ProxyRevoker(proxy) => {
                self.with_roots(|heap| heap.revoke_proxy(proxy))?;
                Ok(Value::Undefined)
            }
            NativeFunction::Map => self.collection_constructor(true, construct),
            NativeFunction::Set => self.collection_constructor(false, construct),
            NativeFunction::WeakMap => self.weak_collection_constructor(true, &args, construct),
            NativeFunction::WeakSet => self.weak_collection_constructor(false, &args, construct),
            NativeFunction::WeakRef => self.weak_ref_constructor(first.clone(), construct),
            NativeFunction::WeakRefDeref => self.weak_ref_deref(&receiver),
            NativeFunction::FinalizationRegistry => {
                self.finalization_registry_constructor(first.clone(), construct)
            }
            NativeFunction::FinalizationRegistryRegister => {
                self.finalization_registry_register(&receiver, &args)
            }
            NativeFunction::FinalizationRegistryUnregister => {
                self.finalization_registry_unregister(&receiver, first.clone())
            }
            NativeFunction::WeakCollectionMethod { map, method } => {
                self.weak_collection_method(map, method, &receiver, &args)
            }
            NativeFunction::ArrayIsArray => Ok(Value::Bool(
                first
                    .object_id()
                    .is_some_and(|id| self.heap.is_array(id).unwrap_or(false)),
            )),
            NativeFunction::ArrayAt => self.array_at(&receiver, first),
            NativeFunction::ArrayOf => self.array_of_method(&receiver, &args),
            NativeFunction::ArraySpecies => Ok(receiver),
            NativeFunction::ArrayFrom => self.array_from_method(&args),
            NativeFunction::ArrayForEach => {
                self.array_for_each(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayFilter => {
                self.array_filter(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayMap => {
                self.array_map(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayFind => {
                self.array_find(&receiver, first, native::argument(&args, 1), false, false)
            }
            NativeFunction::ArrayFindIndex => {
                self.array_find(&receiver, first, native::argument(&args, 1), false, true)
            }
            NativeFunction::ArrayFindLast => {
                self.array_find(&receiver, first, native::argument(&args, 1), true, false)
            }
            NativeFunction::ArrayFindLastIndex => {
                self.array_find(&receiver, first, native::argument(&args, 1), true, true)
            }
            NativeFunction::ArrayEvery => {
                self.array_every(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArraySome => {
                self.array_some(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayIncludes => {
                self.array_includes(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayReduce => self.array_reduce(&receiver, &args),
            NativeFunction::ArrayReduceRight => self.array_reduce_right(&receiver, &args),
            NativeFunction::ArrayPush => {
                let object = self.coerce_object(&receiver)?;
                let array = Value::Object(object);
                self.stack.push(array.clone());
                let result = (|| {
                    for value in &args {
                        self.array_push(&array, value, 0)?;
                    }
                    self.heap.get(object, "length").map_err(Into::into)
                })();
                self.stack.pop();
                result
            }
            NativeFunction::ArrayPop => self.array_pop(&receiver),
            NativeFunction::ArrayShift => self.array_shift(&receiver),
            NativeFunction::ArrayUnshift => self.array_unshift(&receiver, &args),
            NativeFunction::ArrayReverse => self.array_reverse(&receiver),
            NativeFunction::ArrayIndexOf => {
                self.array_index_of(&receiver, first, native::argument(&args, 1))
            }
            NativeFunction::ArrayLastIndexOf => self.array_last_index_of(&receiver, first, &args),
            NativeFunction::ArraySlice => self.array_slice(&receiver, &args),
            NativeFunction::ArraySplice => self.array_splice(&receiver, &args),
            NativeFunction::ArraySort => self.array_sort(&receiver, first),
            NativeFunction::ArrayToLocaleString => self.array_to_locale_string(&receiver),
            NativeFunction::NumberMethod(method) => self.number_method(&receiver, &args, method),
            NativeFunction::Eval => self.indirect_eval(first),
            NativeFunction::IsNaN => Ok(Value::Bool(self.coerce_number(first)?.is_nan())),
            NativeFunction::IsFinite => Ok(Value::Bool(self.coerce_number(first)?.is_finite())),
            NativeFunction::ParseInt => self.parse_int(first, native::argument(&args, 1)),
            NativeFunction::ParseFloat => self.parse_float(first),
            NativeFunction::EncodeUri { component } => self.encode_uri(first, component),
            NativeFunction::DecodeUri { component } => self.decode_uri(first, component),
            NativeFunction::DynamicImport { source } => {
                if source {
                    self.dynamic_import_source(first.clone())
                } else {
                    self.dynamic_import(first.clone())
                }
            }
            NativeFunction::JsonParse => self.json_parse(first),
            NativeFunction::JsonStringify => self.json_stringify(first),
            NativeFunction::Math(method) => self.math_method(method, &args),
            NativeFunction::Bind => self.bind_function(receiver, &args),
            NativeFunction::HasInstance => self
                .has_instance(first.clone(), receiver, true)
                .map(Value::Bool),
            NativeFunction::RegExpEscape => self.regexp_escape(first),
            NativeFunction::ArrayIterator(kind) => {
                let object = self.coerce_object(&receiver)?;
                let prototype = self.array_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_array_iterator(object, kind, prototype)
                })?))
            }
            NativeFunction::ArrayIteratorNext => {
                let Value::Object(id) = receiver else {
                    return Err(RuntimeError::TypeError(
                        "Array iterator next requires an iterator".into(),
                    ));
                };
                let Some((object, index, done, kind)) = self.heap.array_iterator(id)? else {
                    return Err(RuntimeError::TypeError(
                        "Array iterator next requires an iterator".into(),
                    ));
                };
                if done {
                    return self.iterator_result(Value::Undefined, true);
                }
                let length = self.get_property(&Value::Object(object), &"length".into())?;
                let length = self.coerce_length(&length)?;
                let done = index as f64 >= length;
                self.heap.advance_array_iterator(id, done);
                let value = if done {
                    Value::Undefined
                } else {
                    match kind {
                        ArrayIteratorKind::Keys => Value::Number(index as f64),
                        ArrayIteratorKind::Values => {
                            self.get_property(&Value::Object(object), &index.to_string().into())?
                        }
                        ArrayIteratorKind::Entries => {
                            let entry = self
                                .get_property(&Value::Object(object), &index.to_string().into())?;
                            self.array_from(vec![Value::Number(index as f64), entry])?
                        }
                    }
                };
                self.iterator_result(value, done)
            }
            NativeFunction::CollectionIterator { map } => self.collection_iterator(map, &receiver),
            NativeFunction::CollectionIteratorNext => self.iterator_result(Value::Undefined, true),
            NativeFunction::GeneratorNext => {
                self.generator_next(&receiver, Some(first.clone()), None)
            }
            NativeFunction::GeneratorReturn => self.generator_return(&receiver, first.clone()),
            NativeFunction::GeneratorThrow => self.generator_throw(&receiver, first.clone()),
            NativeFunction::AsyncGeneratorNext
            | NativeFunction::AsyncGeneratorReturn
            | NativeFunction::AsyncGeneratorThrow => {
                self.async_generator_request(&receiver, first.clone(), function)
            }
            NativeFunction::Apply => {
                if !self.is_callable(&receiver)? {
                    return Err(RuntimeError::TypeError("apply requires a callable".into()));
                }
                let list = native::argument(&args, 1);
                let values = if matches!(list, Value::Null | Value::Undefined) {
                    Vec::new()
                } else {
                    self.array_like_values(list)?
                };
                self.call_native(receiver, first.clone(), values, false)
            }
            NativeFunction::ReflectApply => {
                if !self.is_callable(first)? {
                    return Err(RuntimeError::TypeError(
                        "Reflect.apply requires a callable target".into(),
                    ));
                }
                let values = self.array_like_values(native::argument(&args, 2))?;
                self.call_native(
                    first.clone(),
                    native::argument(&args, 1).clone(),
                    values,
                    false,
                )
            }
            NativeFunction::ReflectConstruct => {
                let new_target = if args.len() > 2 {
                    args[2].clone()
                } else {
                    first.clone()
                };
                if !self.is_constructor(first)? || !self.is_constructor(&new_target)? {
                    return Err(RuntimeError::TypeError(
                        "Reflect.construct requires constructors".into(),
                    ));
                }
                let values = self.array_like_values(native::argument(&args, 1))?;
                self.call_with_target(first.clone(), Value::Undefined, values, true, new_target)
            }
            NativeFunction::FunctionToString => {
                if !self.is_callable(&receiver)? {
                    return Err(RuntimeError::TypeError(
                        "Function.toString requires a callable".into(),
                    ));
                }
                let name = self
                    .heap
                    .function_initial_name(receiver.object_id().unwrap())?;
                let mut result = JsString::from("function ");
                result.push_str(&name);
                result.push_str(&"() { [native code] }".into());
                Ok(Value::String(result))
            }
            NativeFunction::PrimitiveConstructor(boolean) => {
                let value = if boolean {
                    Value::Bool(self.to_boolean(first)?)
                } else if let Value::BigInt(value) = first {
                    Value::Number(value.to_f64().unwrap_or_else(|| {
                        if value.sign() == Sign::Minus {
                            f64::NEG_INFINITY
                        } else {
                            f64::INFINITY
                        }
                    }))
                } else {
                    Value::Number(if args.is_empty() {
                        0.0
                    } else {
                        self.coerce_number(first)?
                    })
                };
                if !construct {
                    return Ok(value);
                }
                let constructor = self.global(if boolean { "Boolean" } else { "Number" })?;
                let default = self
                    .get_property(&constructor, &"prototype".into())?
                    .object_id()
                    .unwrap();
                let prototype = self.constructor_prototype(default)?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_boxed_primitive(value, prototype)
                })?))
            }
            NativeFunction::BigInt => {
                if construct {
                    return Err(RuntimeError::TypeError(
                        "BigInt is not a constructor".into(),
                    ));
                }
                let value = self.coerce_primitive(first, "number")?;
                match value {
                    Value::BigInt(value) => Ok(Value::BigInt(value)),
                    Value::Number(value) if value.is_finite() && value.fract() == 0.0 => {
                        // Every integral IEEE-754 Number is within i64's
                        // magnitude range, including the safe-integer range
                        // used by TypedArray conversion fixtures.
                        Ok(Value::BigInt(BigInt::from(value as i64)))
                    }
                    Value::Number(_) => Err(RuntimeError::RangeError(
                        "BigInt conversion requires an integral Number".into(),
                    )),
                    Value::String(value) => {
                        let value = value.to_utf8().map_err(|_| {
                            RuntimeError::SyntaxError("invalid BigInt string".into())
                        })?;
                        let value =
                            BigInt::parse_bytes(value.trim().as_bytes(), 10).ok_or_else(|| {
                                RuntimeError::SyntaxError("invalid BigInt string".into())
                            })?;
                        Ok(Value::BigInt(value))
                    }
                    _ => Err(RuntimeError::TypeError(
                        "BigInt conversion requires a Number, BigInt, or integer string".into(),
                    )),
                }
            }
            NativeFunction::PrimitiveMethod { boolean, string } => {
                let value = if let Value::Object(id) = receiver {
                    self.heap.boxed_primitive(id)?.unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                if !matches!(
                    (&value, boolean),
                    (Value::Bool(_), true) | (Value::Number(_), false)
                ) {
                    return Err(RuntimeError::TypeError(
                        "incompatible boxed primitive receiver".into(),
                    ));
                }
                if string {
                    Ok(Value::String(primitive::string(&value)?))
                } else {
                    Ok(value)
                }
            }
            NativeFunction::SymbolToString | NativeFunction::SymbolValueOf => {
                let value = if let Value::Object(id) = receiver {
                    self.heap
                        .boxed_primitive(id)?
                        .or(self.test262_foreign_boxed_primitive(id)?)
                        .unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                let Value::Symbol(symbol) = value else {
                    return Err(RuntimeError::TypeError(
                        "Symbol method requires a Symbol".into(),
                    ));
                };
                if function == NativeFunction::SymbolToString {
                    Ok(Value::String(symbol.descriptive_string()))
                } else {
                    Ok(Value::Symbol(symbol))
                }
            }
            NativeFunction::BigIntToString | NativeFunction::BigIntValueOf => {
                let value = if let Value::Object(id) = receiver {
                    self.heap.boxed_primitive(id)?.unwrap_or(Value::Undefined)
                } else {
                    receiver
                };
                let Value::BigInt(value) = value else {
                    return Err(RuntimeError::TypeError(
                        "BigInt method requires a BigInt".into(),
                    ));
                };
                if function == NativeFunction::BigIntToString {
                    Ok(Value::String(value.to_string().into()))
                } else {
                    Ok(Value::BigInt(value))
                }
            }
            NativeFunction::RegExp => {
                if !construct
                    && *native::argument(&args, 1) == Value::Undefined
                    && self.is_regexp(first)?
                {
                    let constructor = self.get_property(first, &"constructor".into())?;
                    if constructor == self.regexp_global()? {
                        return Ok(first.clone());
                    }
                }
                self.regexp_create(first, native::argument(&args, 1))
            }
            NativeFunction::RegExpMethod(method) => self.regexp_method(method, &receiver, &args),
            NativeFunction::RegExpGetter(name) => self.regexp_getter(name, &receiver),
            NativeFunction::RegExpIteratorNext => self.regexp_iterator_next(&receiver),
            NativeFunction::ThrowTypeError => Err(RuntimeError::TypeError(
                "restricted function property".into(),
            )),
            NativeFunction::Empty => Ok(Value::Undefined),
            NativeFunction::ObjectValueOf => self.coerce_object(&receiver).map(Value::Object),
            NativeFunction::ObjectIsPrototypeOf => {
                // §20.1.3.6 tests the argument before coercing `this`.  That
                // ordering keeps primitive arguments observable as `false`,
                // even when `this` is null or undefined.
                let Value::Object(mut candidate) = first else {
                    return Ok(Value::Bool(false));
                };
                let object = self.coerce_object(&receiver)?;
                let base = self.stack.len();
                self.stack
                    .extend([Value::Object(object), Value::Object(candidate)]);
                let result = (|| {
                    while let Some(prototype) = self.object_get_prototype(candidate)? {
                        if prototype == object {
                            return Ok(Value::Bool(true));
                        }
                        candidate = prototype;
                        *self.stack.last_mut().expect("prototype-chain root") =
                            Value::Object(candidate);
                    }
                    Ok(Value::Bool(false))
                })();
                self.stack.truncate(base);
                result
            }
            NativeFunction::ObjectDefineAccessor { getter } => {
                // Annex B.2.2.2/B.2.2.3: establish that the receiver and
                // accessor are usable before observing a coercible key. The
                // roots remain live while ToPropertyKey and a Proxy's
                // [[DefineOwnProperty]] trap can re-enter JavaScript.
                let object = self.coerce_object(&receiver)?;
                let key_value = native::argument(&args, 0).clone();
                let accessor = native::argument(&args, 1).clone();
                let base = self.stack.len();
                self.stack
                    .extend([Value::Object(object), key_value.clone(), accessor.clone()]);
                let result = (|| {
                    if !self.is_callable(&accessor)? {
                        return Err(RuntimeError::TypeError(
                            "legacy accessor must be callable".into(),
                        ));
                    }
                    let key = self.coerce_property_key(&key_value)?;
                    let descriptor = if getter {
                        PropertyDescriptor {
                            get: Some(accessor),
                            enumerable: Some(true),
                            configurable: Some(true),
                            ..PropertyDescriptor::default()
                        }
                    } else {
                        PropertyDescriptor {
                            set: Some(accessor),
                            enumerable: Some(true),
                            configurable: Some(true),
                            ..PropertyDescriptor::default()
                        }
                    };
                    if !self.object_define_own_property(object, key, descriptor)? {
                        return Err(RuntimeError::TypeError(
                            "cannot define legacy accessor".into(),
                        ));
                    }
                    Ok(Value::Undefined)
                })();
                self.stack.truncate(base);
                result
            }
            NativeFunction::ObjectLookupAccessor { getter } => {
                // Annex B.2.2.4/B.2.2.5 deliberately use [[GetOwnProperty]]
                // and [[GetPrototypeOf]], so Proxy traps and their abrupt
                // completions cannot be skipped by an ordinary heap walk.
                let object = self.coerce_object(&receiver)?;
                let key_value = native::argument(&args, 0).clone();
                let base = self.stack.len();
                self.stack
                    .extend([Value::Object(object), key_value.clone()]);
                let result = (|| {
                    let key = self.coerce_property_key(&key_value)?;
                    let mut current = object;
                    loop {
                        if let Some(descriptor) = self.object_get_own_property(current, &key)? {
                            return Ok(if getter {
                                descriptor.get.unwrap_or(Value::Undefined)
                            } else {
                                descriptor.set.unwrap_or(Value::Undefined)
                            });
                        }
                        let Some(prototype) = self.object_get_prototype(current)? else {
                            return Ok(Value::Undefined);
                        };
                        current = prototype;
                        self.stack[base] = Value::Object(current);
                    }
                })();
                self.stack.truncate(base);
                result
            }
            NativeFunction::ObjectPrototypeGetter => {
                let object = self.coerce_object(&receiver)?;
                let base = self.stack.len();
                self.stack.push(Value::Object(object));
                let result = self
                    .object_get_prototype(object)
                    .map(|prototype| prototype.map_or(Value::Null, Value::Object));
                self.stack.truncate(base);
                result
            }
            NativeFunction::ObjectPrototypeSetter => {
                if matches!(receiver, Value::Null | Value::Undefined) {
                    return Err(RuntimeError::TypeError(
                        "cannot convert null or undefined to Object".into(),
                    ));
                }
                let prototype = match native::argument(&args, 0) {
                    Value::Object(prototype) => Some(*prototype),
                    Value::Null => None,
                    _ => return Ok(Value::Undefined),
                };
                let Value::Object(object) = receiver else {
                    return Ok(Value::Undefined);
                };
                let base = self.stack.len();
                self.stack.push(Value::Object(object));
                if let Some(prototype) = prototype {
                    self.stack.push(Value::Object(prototype));
                }
                let result = (|| {
                    if !self.object_set_prototype(object, prototype)? {
                        return Err(RuntimeError::TypeError(
                            "cannot set object prototype".into(),
                        ));
                    }
                    Ok(Value::Undefined)
                })();
                self.stack.truncate(base);
                result
            }
            NativeFunction::ObjectToString => {
                let tag = match &receiver {
                    Value::Undefined => "Undefined",
                    Value::Null => "Null",
                    Value::String(_) => "String",
                    // Symbol and BigInt obtain their default tag from their
                    // prototypes' @@toStringTag properties.  If user code
                    // replaces those with a non-string value, the ordinary
                    // fallback is Object rather than a hidden primitive tag.
                    Value::Symbol(_) => "Object",
                    Value::Number(_) => "Number",
                    Value::BigInt(_) => "Object",
                    Value::Bool(_) => "Boolean",
                    Value::Object(id) => {
                        // IsArray walks Proxy targets and throws for a
                        // revoked Proxy before Object.prototype.toString
                        // observes @@toStringTag.  The rest of BlueJS's
                        // object brands remain heap-owned, so unwrap only
                        // for this internal-slot inspection while retaining
                        // the original receiver for the later Get.
                        let mut branded = *id;
                        while let Some((target, _)) = self.heap.proxy(branded)? {
                            branded = target;
                        }
                        if self.heap.boxed_string(branded)?.is_some() {
                            "String"
                        } else if self.heap.is_array(branded)? {
                            "Array"
                        } else if self.heap.is_arguments(branded)? {
                            "Arguments"
                        } else if self.heap.is_date(branded)? {
                            "Date"
                        } else if self.is_callable(&receiver)? {
                            "Function"
                        } else if self.heap.regexp(branded)?.is_some() {
                            "RegExp"
                        } else if self.heap.is_error(branded)? {
                            "Error"
                        } else if let Some(value) = self.heap.boxed_primitive(branded)? {
                            match value {
                                Value::Number(_) => "Number",
                                Value::Bool(_) => "Boolean",
                                Value::BigInt(_) | Value::Symbol(_) => "Object",
                                _ => "Object",
                            }
                        } else {
                            "Object"
                        }
                    }
                };
                let custom = if matches!(receiver, Value::Undefined | Value::Null) {
                    Value::Undefined
                } else {
                    self.get_property(&receiver, &JsSymbol::well_known("toStringTag").into())?
                };
                let mut result = JsString::from("[object ");
                result.push_str(&if let Value::String(custom) = custom {
                    custom
                } else {
                    tag.into()
                });
                result.push_str(&"]".into());
                Ok(Value::String(result))
            }
            NativeFunction::ObjectToLocaleString => {
                if matches!(receiver, Value::Null | Value::Undefined) {
                    return Err(RuntimeError::TypeError(
                        "cannot convert null or undefined to Object".into(),
                    ));
                }
                let base = self.stack.len();
                self.stack.push(receiver.clone());
                let result = (|| {
                    let to_string = self.get_property(&receiver, &"toString".into())?;
                    self.stack.push(to_string.clone());
                    if !self.is_callable(&to_string)? {
                        return Err(RuntimeError::TypeError(
                            "toString property is not callable".into(),
                        ));
                    }
                    self.call_native(to_string, receiver, vec![], false)
                })();
                self.stack.truncate(base);
                result
            }
            NativeFunction::ArrayToString => {
                let object = Value::Object(self.coerce_object(&receiver)?);
                self.stack.push(object.clone());
                let join = self.get_property(&object, &"join".into())?;
                if self.is_callable(&join)? {
                    self.call_native(join, object, vec![], false)
                } else {
                    self.native_call(NativeFunction::ObjectToString, object, vec![], false)
                }
            }
            NativeFunction::ArrayConcat => self.array_concat(&receiver, &args),
            NativeFunction::ArrayJoin => self.array_join(&receiver, first),
            NativeFunction::Symbol => Ok(Value::Symbol(JsSymbol::new(
                if matches!(first, Value::Undefined) {
                    None
                } else {
                    Some(self.coerce_string(first)?)
                },
            ))),
            NativeFunction::SymbolFor => {
                let key = self.coerce_string(first)?;
                let symbol = self
                    .symbol_registry
                    .entry(key.clone())
                    .or_insert_with(|| JsSymbol::new(Some(key)))
                    .clone();
                Ok(Value::Symbol(symbol))
            }
            NativeFunction::SymbolKeyFor => {
                let Value::Symbol(symbol) = first else {
                    return Err(RuntimeError::TypeError(
                        "Symbol.keyFor requires a Symbol".into(),
                    ));
                };
                Ok(self
                    .symbol_registry
                    .iter()
                    .find_map(|(key, candidate)| (candidate == symbol).then(|| key.clone()))
                    .map_or(Value::Undefined, Value::String))
            }
            NativeFunction::Object => {
                // Object(value) normally returns an object argument (or
                // boxes a primitive), but a distinct NewTarget takes the
                // OrdinaryCreateFromConstructor branch first.  This is what
                // makes `class C extends Object {}` and
                // Reflect.construct(Object, values, C) allocate a fresh C
                // instance rather than returning `values[0]`.
                let object_constructor = self.global("Object")?;
                if construct && self.new_target != object_constructor {
                    let prototype = self.constructor_prototype(self.object_prototype)?;
                    return Ok(Value::Object(
                        self.with_roots(|heap| heap.alloc_object(Some(prototype)))?,
                    ));
                }
                if matches!(first, Value::Undefined | Value::Null) {
                    let proto = if construct {
                        self.constructor_prototype(self.object_prototype)?
                    } else {
                        self.object_prototype
                    };
                    return Ok(Value::Object(
                        self.with_roots(|heap| heap.alloc_object(Some(proto)))?,
                    ));
                }
                self.coerce_object(first).map(Value::Object)
            }
            NativeFunction::ObjectMethod(method) => self.object_method(method, &receiver, &args),
            NativeFunction::StringIterator => {
                let string = self.string_receiver(&receiver)?;
                let prototype = self.string_iterator_prototype()?;
                Ok(Value::Object(self.with_roots(|heap| {
                    heap.alloc_string_iterator(string, prototype)
                })?))
            }
            NativeFunction::IteratorNext => {
                let Value::Object(id) = receiver else {
                    return Err(RuntimeError::TypeError(
                        "iterator next requires an iterator".into(),
                    ));
                };
                let Some(value) = self.heap.string_iterator_next(id)? else {
                    return Err(RuntimeError::TypeError(
                        "iterator next requires a String iterator".into(),
                    ));
                };
                let done = value.is_none();
                self.iterator_result(value.map_or(Value::Undefined, Value::String), done)
            }
            NativeFunction::IteratorSelf | NativeFunction::AsyncIteratorSelf => Ok(receiver),
            NativeFunction::Pattern(method) => self.string_pattern(method, &receiver, &args),
            NativeFunction::String => {
                let string = if args.is_empty() {
                    JsString::default()
                } else {
                    self.string_constructor_argument(native::argument(&args, 0), construct)?
                };
                self.check_string(&Value::String(string.clone()))?;
                if construct {
                    let (_, prototype) = self.string_intrinsics()?;
                    let prototype = self.constructor_prototype(prototype)?;
                    Ok(Value::Object(self.with_roots(|heap| {
                        heap.alloc_string(string, Some(prototype))
                    })?))
                } else {
                    Ok(Value::String(string))
                }
            }
            NativeFunction::FromCharCode | NativeFunction::FromCodePoint => {
                let mut result = JsString::default();
                for arg in &args {
                    let number = Value::Number(self.coerce_number(arg)?);
                    let Value::String(part) = native::from_codes(
                        &[number],
                        function == NativeFunction::FromCodePoint,
                        self.config.max_string_bytes,
                    )?
                    else {
                        unreachable!()
                    };
                    native::append(&mut result, &part, self.config.max_string_bytes)?;
                }
                Ok(Value::String(result))
            }
            NativeFunction::Raw => self.string_raw(&args),
            NativeFunction::Split => self.string_split(&receiver, &args),
            NativeFunction::Replace | NativeFunction::ReplaceAll => {
                self.string_replace(&receiver, &args, function == NativeFunction::ReplaceAll)
            }
            NativeFunction::StringMethod(method) => {
                self.dispatch_string_method(method, &receiver, &args)
            }
            NativeFunction::Call => self.call_native(
                receiver,
                first.clone(),
                args.iter().skip(1).cloned().collect(),
                false,
            ),
        }
    }
}
