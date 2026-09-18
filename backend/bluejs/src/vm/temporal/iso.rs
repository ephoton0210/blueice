// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Host-neutral ISO 8601 / Temporal grammar parsing.
//!
//! Every function here is plain Rust with no `Value`/heap/Realm coupling —
//! directly unit-testable without a VM, per Phase 26's foundation/adapter
//! split (`development/browser_core/phase-26-ecma262-temporal/PLAN.md`).
//! `vm/temporal.rs`'s `impl Vm` methods are the only callers that bridge
//! these results into JavaScript-visible values.
//!
//! This is a strict recursive-descent scan of Temporal's own grammar
//! (`ISODateTime`, `AnnotatedDateTime`, `TemporalYearMonthString`,
//! `TemporalMonthDayString`, `TemporalTimeString`, `TemporalDurationString`),
//! not a lenient split-on-separators approximation:
//!
//! - Both the extended (`1976-11-18`, `15:23:30`) and basic (`19761118`,
//!   `152330`) forms are accepted, and the separator choice must be
//!   consistent within one date or one time (`2020-0101` and `00:0000` are
//!   syntax errors).
//! - Every numeric field has an exact digit count: a year is 4 digits, or a
//!   sign plus exactly 6 (`+002020`, with `-000000` rejected outright);
//!   months, days, hours, minutes and seconds are exactly 2.
//! - A decimal fraction may be written with `.` or `,` and holds 1 to 9
//!   digits — a 10th digit is a syntax error rather than silently truncated.
//! - The date/time separator is `T`, `t` or a single space.
//! - A `60` seconds field (a leap second) parses and is clamped to `59`.
//! - A UTC offset may carry sub-minute precision with its own fraction
//!   (`+00:19:32.37`), so offsets are returned in nanoseconds; a time-zone
//!   *annotation*, by contrast, is restricted to minute precision.
//! - Parsing never applies Temporal's per-type representable-range limits:
//!   the same `-999999-10-01` that is out of range for a `PlainDate` is a
//!   perfectly valid `PlainMonthDay` string. Callers apply the range rule
//!   their type actually has (see `epoch::is_date_time_within_limits` and
//!   `is_year_month_within_limits`).
//!
//! The rules above are read off the pinned Test262 corpus rather than
//! assumed; the unit tests below cite the fixture each case comes from.

/// A time of day: `(hour, minute, second, millisecond, microsecond,
/// nanosecond)`.
pub(crate) type Time = (u8, u8, u8, u16, u16, u16);

/// The result of scanning one Temporal date/time string.
///
/// Which fields are meaningful depends on the entry point that produced it:
/// [`parse_year_month`] can report a source that never spelled a day, and
/// [`parse_month_day`] one that never spelled a year, so the presence flags
/// are part of the result rather than something a caller can re-derive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Parsed {
    pub(crate) year: i32,
    pub(crate) month: u8,
    pub(crate) day: u8,
    /// Whether the source spelled a year (false for `--11-18`/`11-18`).
    pub(crate) year_present: bool,
    /// Whether the source spelled a day (false for `1976-11`).
    pub(crate) day_present: bool,
    pub(crate) time: Option<Time>,
    /// Whether the source ended its time with the UTC designator `Z`/`z`.
    pub(crate) utc_designator: bool,
    /// A numeric UTC offset, in nanoseconds east of UTC.
    pub(crate) offset_nanoseconds: Option<i64>,
    /// The time-zone annotation's identifier, if one was present.
    pub(crate) time_zone: Option<String>,
    /// The first `u-ca=` annotation's value, if one was present.
    pub(crate) calendar: Option<String>,
}

impl Default for Parsed {
    fn default() -> Self {
        Self {
            year: 1970,
            month: 1,
            day: 1,
            year_present: false,
            day_present: false,
            time: None,
            utc_designator: false,
            offset_nanoseconds: None,
            time_zone: None,
            calendar: None,
        }
    }
}

use super::epoch::{CivilDate, CivilTime};
use num_bigint::BigInt;

/// Returns whether `year` is a leap year in the proleptic Gregorian
/// calendar (the ISO 8601 calendar).
pub(crate) fn is_leap_year(year: i32) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

/// Returns the number of days in `month` of `year`, or `None` for a month
/// outside `1..=12`.
pub(crate) fn days_in_month(year: i32, month: u8) -> Option<u8> {
    Some(match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return None,
    })
}

/// `ISOYearMonthWithinLimits`: the representable range of a
/// `Temporal.PlainYearMonth`, which is one month wider at each end than the
/// `PlainDate` range so that every in-range date's year-month is itself
/// in range.
pub(crate) fn is_year_month_within_limits(year: i32, month: u8) -> bool {
    match year {
        -271_821 => month >= 4,
        275_760 => month <= 9,
        _ => (-271_821..=275_760).contains(&year),
    }
}

/// A byte cursor over an ISO string. Every character in Temporal's grammar
/// is ASCII, so byte-wise scanning both matches the grammar exactly and
/// rejects look-alikes such as the Unicode minus sign U+2212 for free.
struct Cursor<'a> {
    source: &'a str,
    index: usize,
}

impl<'a> Cursor<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, index: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.index).copied()
    }

    fn peek_digit(&self) -> bool {
        self.peek().is_some_and(|byte| byte.is_ascii_digit())
    }

    fn done(&self) -> bool {
        self.index >= self.source.len()
    }

    fn eat(&mut self, byte: u8) -> bool {
        let matched = self.peek() == Some(byte);
        if matched {
            self.index += 1;
        }
        matched
    }

    fn eat_any(&mut self, bytes: &[u8]) -> Option<u8> {
        let byte = self.peek()?;
        bytes.contains(&byte).then(|| {
            self.index += 1;
            byte
        })
    }

    /// Consumes exactly `count` ASCII digits, or nothing.
    fn digits(&mut self, count: usize) -> Option<u32> {
        let text = self.source.get(self.index..self.index + count)?;
        if !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        self.index += count;
        text.parse().ok()
    }

    /// Consumes a `TemporalDecimalFraction`'s digits (1 to 9 of them, a
    /// 10th being a syntax error) and returns them scaled to nanoseconds.
    fn fraction_nanoseconds(&mut self) -> Option<u32> {
        let start = self.index;
        while self.peek_digit() {
            self.index += 1;
        }
        let digits = &self.source[start..self.index];
        if digits.is_empty() || digits.len() > 9 {
            return None;
        }
        let mut nanoseconds: u32 = digits.parse().ok()?;
        for _ in digits.len()..9 {
            nanoseconds *= 10;
        }
        Some(nanoseconds)
    }

    /// Reads the body of the bracketed annotation at the cursor without
    /// consuming it.
    fn peek_bracket(&self) -> Option<&'a str> {
        let rest = self.source.get(self.index..)?.strip_prefix('[')?;
        rest.find(']').map(|end| &rest[..end])
    }

    /// Consumes the bracketed annotation at the cursor, returning its body.
    fn take_bracket(&mut self) -> Option<&'a str> {
        let body = self.peek_bracket()?;
        self.index += body.len() + 2;
        Some(body)
    }
}

/// `DateYear`: 4 digits, or a sign and exactly 6. A signed zero year is
/// rejected (`Temporal/PlainDate/from/year-zero.js`).
fn scan_year(cursor: &mut Cursor) -> Option<i32> {
    match cursor.peek() {
        Some(sign @ (b'+' | b'-')) => {
            cursor.index += 1;
            let value = cursor.digits(6)? as i32;
            if sign == b'-' {
                (value != 0).then_some(-value)
            } else {
                Some(value)
            }
        }
        _ => cursor.digits(4).map(|value| value as i32),
    }
}

/// `ISODate`: `DateYear ['-'] DateMonth ['-'] DateDay`, with the same
/// separator choice on both sides.
fn scan_date(cursor: &mut Cursor) -> Option<(i32, u8, u8)> {
    let year = scan_year(cursor)?;
    let extended = cursor.eat(b'-');
    let month = cursor.digits(2)? as u8;
    if cursor.eat(b'-') != extended {
        return None;
    }
    let day = cursor.digits(2)? as u8;
    is_valid_date(year, month, day).then_some((year, month, day))
}

fn is_valid_date(year: i32, month: u8, day: u8) -> bool {
    day >= 1 && days_in_month(year, month).is_some_and(|last| day <= last)
}

/// `TimeSpec`: `Hour [[':'] Minute [[':'] Second [Fraction]]]`, with the
/// separator choice consistent throughout. A `60` seconds field is a leap
/// second and parses as `59`, per `ParseISODateTime`.
fn scan_time(cursor: &mut Cursor) -> Option<Time> {
    let hour = cursor.digits(2)?;
    if hour > 23 {
        return None;
    }
    let mut minute = 0;
    let mut second = 0;
    let mut nanoseconds = 0;
    let extended = cursor.eat(b':');
    if extended || cursor.peek_digit() {
        minute = cursor.digits(2)?;
        if minute > 59 {
            return None;
        }
        let seconds_follow = if extended {
            cursor.eat(b':')
        } else {
            cursor.peek_digit()
        };
        if seconds_follow {
            second = cursor.digits(2)?;
            if second > 60 {
                return None;
            }
            second = second.min(59);
            if cursor.eat_any(b".,").is_some() {
                nanoseconds = cursor.fraction_nanoseconds()?;
            }
        }
    }
    Some((
        hour as u8,
        minute as u8,
        second as u8,
        (nanoseconds / 1_000_000) as u16,
        ((nanoseconds / 1_000) % 1_000) as u16,
        (nanoseconds % 1_000) as u16,
    ))
}

fn two_digit_field(text: &str) -> Option<u8> {
    (text.len() == 2 && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().expect("two ASCII digits always fit in a u8"))
}

/// Parses `Hour[[:]Minute[[:]Second[('.'|',')Fraction]]]`, returning
/// `(hour, minute, second, nanoseconds)`.
///
/// Shared by the time-of-day parser and the UTC-offset parser, which have
/// the same shape in Temporal's grammar. Two rules it enforces that a
/// field-splitting parser easily misses: the `:` separators are either all
/// present or all absent (`00:0000` mixes the two and is invalid), and a
/// fraction belongs to the *seconds* field only — so `05:07.123`
/// (fractional minutes) and `12.5` (fractional hours) are syntax errors,
/// not roundable values.
fn parse_time_spec(source: &str) -> Option<(u8, u8, u8, u32)> {
    let (body, fraction) = match source.split_once(['.', ',']) {
        Some((body, fraction)) => (body, Some(fraction)),
        None => (source, None),
    };
    let (hour, minute, second) = if body.contains(':') {
        let mut fields = body.split(':');
        let hour = two_digit_field(fields.next()?)?;
        let minute = two_digit_field(fields.next()?)?;
        let second = match fields.next() {
            Some(field) => Some(two_digit_field(field)?),
            None => None,
        };
        if fields.next().is_some() {
            return None;
        }
        (hour, minute, second)
    } else {
        match body.len() {
            2 => (two_digit_field(body)?, 0, None),
            4 => (
                two_digit_field(&body[..2])?,
                two_digit_field(&body[2..])?,
                None,
            ),
            6 => (
                two_digit_field(&body[..2])?,
                two_digit_field(&body[2..4])?,
                Some(two_digit_field(&body[4..])?),
            ),
            _ => return None,
        }
    };
    let nanoseconds = match (second, fraction) {
        (_, None) => 0,
        (None, Some(_)) => return None,
        (Some(_), Some(fraction)) => {
            if fraction.is_empty()
                || fraction.len() > 9
                || !fraction.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            let mut nanos = fraction
                .bytes()
                .fold(0u32, |value, byte| value * 10 + u32::from(byte - b'0'));
            for _ in fraction.len()..9 {
                nanos *= 10;
            }
            nanos
        }
    };
    (hour <= 23 && minute <= 59).then_some((hour, minute, second.unwrap_or(0), nanoseconds))
}

/// Parses the whole `YYYY-MM-DD` date portion at the start of an ISO
/// date/date-time/instant string, in either the extended (separated) or the
/// basic (separator-less) form, rejecting anything left over before the time
/// designator, `Z`, or annotation.
pub(crate) fn parse_date(source: &str) -> Option<(i32, u8, u8)> {
    let end = source
        .find(['T', 't', '[', 'Z', 'z'])
        .unwrap_or(source.len());
    let (date, rest) = parse_iso_date_prefix(&source[..end])?;
    rest.is_empty().then_some(date)
}

/// Parses the UTC offset (or `Z`/`z` designator) found within `source` into
/// signed whole seconds, dropping any sub-second fraction: no real zone
/// offset has sub-second precision, and `Temporal.PlainTime` ignores the
/// offset entirely — it is parsed only so a malformed one is still a syntax
/// error.
pub(crate) fn parse_offset_seconds(source: &str) -> Option<i32> {
    let index = source.find(['Z', 'z', '+', '-', '['])?;
    let (offset, rest) = parse_utc_offset_prefix(&source[index..])?;
    if !(rest.is_empty() || rest.starts_with('[')) {
        return None;
    }
    i32::try_from(offset.nanoseconds / 1_000_000_000).ok()
}

/// The annotation suffix of an ISO date/time/offset string: an optional
/// leading time-zone annotation followed by zero or more `[key=value]`
/// annotations.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Annotations {
    /// The time-zone annotation's body (without `!`), if one was present.
    pub(crate) time_zone: Option<String>,
    /// The first `u-ca=` annotation's value, if any.
    pub(crate) calendar: Option<String>,
}

/// Scans the zero-or-more bracket annotations that may follow an ISO
/// date/time/offset prefix.
///
/// Grammar notes (from Temporal's annotation syntax): an optional leading
/// time-zone annotation (no `=` in its body) is returned as
/// [`Annotations::time_zone`] after being checked against
/// `TimeZoneIdentifier` — an IANA-shaped name, or a UTC offset with at most
/// minute precision (Test262's `instant-string-sub-minute-offset.js` makes
/// the sub-minute rejection a syntax error, not a later resolution failure).
/// Whether a named identifier actually *exists* is a separate, later concern
/// (Stage 1 Track E's `time_zone.rs`). Every subsequent annotation is
/// `[!]key=value`; a key containing any non-lowercase character is always a
/// syntax error, regardless of the critical (`!`) flag. A second or later
/// `u-ca` annotation is ignored rather than resolved, but more than one
/// `u-ca` annotation with any of them critical is a syntax error. Any other
/// unrecognized key is ignored unless marked critical, in which case this
/// returns `Err`.
pub(crate) fn parse_annotation_suffix(mut cursor: &str) -> Result<Annotations, ()> {
    let mut result = Annotations::default();
    if let Some(rest) = cursor.strip_prefix('[') {
        let end = rest.find(']').ok_or(())?;
        let body = rest[..end].strip_prefix('!').unwrap_or(&rest[..end]);
        if !body.contains('=') {
            if !is_time_zone_identifier(body) {
                return Err(());
            }
            result.time_zone = Some(body.to_string());
            cursor = &rest[end + 1..];
        }
    }
    let mut calendars = 0_usize;
    let mut critical_calendar = false;
    while !cursor.is_empty() {
        let rest = cursor.strip_prefix('[').ok_or(())?;
        let end = rest.find(']').ok_or(())?;
        let body = &rest[..end];
        cursor = &rest[end + 1..];
        let (critical, body) = body
            .strip_prefix('!')
            .map_or((false, body), |rest| (true, rest));
        let (key, value) = body.split_once('=').ok_or(())?;
        if key.is_empty() || value.is_empty() {
            return Err(());
        }
        let key_valid = key.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_lowercase() || byte == b'_'
            } else {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            }
        });
        let value_valid = value.split('-').all(|component| {
            !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_alphanumeric())
        });
        if !key_valid || !value_valid {
            return Err(());
        }
        if key == "u-ca" {
            calendars += 1;
            critical_calendar |= critical;
            if calendars > 1 && critical_calendar {
                return Err(());
            }
            result.calendar.get_or_insert_with(|| value.to_string());
        } else if critical {
            return Err(());
        }
    }
    Ok(result)
}

/// `UTCOffset`, `Cursor`-based: shared by [`scan_utc_offset_suffix`] and
/// [`is_valid_time_zone_identifier`]'s own offset form.
fn scan_offset(cursor: &mut Cursor, sub_minute: bool) -> Option<i64> {
    let sign = match cursor.eat_any(b"+-")? {
        b'-' => -1,
        _ => 1,
    };
    let hour = i64::from(cursor.digits(2)?);
    if hour > 23 {
        return None;
    }
    let mut minute = 0;
    let mut second = 0;
    let mut nanoseconds = 0;
    let extended = cursor.eat(b':');
    if extended || cursor.peek_digit() {
        minute = i64::from(cursor.digits(2)?);
        if minute > 59 {
            return None;
        }
        let seconds_follow = if extended {
            cursor.eat(b':')
        } else {
            cursor.peek_digit()
        };
        if seconds_follow {
            if !sub_minute {
                return None;
            }
            second = i64::from(cursor.digits(2)?);
            if second > 59 {
                return None;
            }
            if cursor.eat_any(b".,").is_some() {
                nanoseconds = i64::from(cursor.fraction_nanoseconds()?);
            }
        }
    }
    Some(sign * (((hour * 60 + minute) * 60 + second) * 1_000_000_000 + nanoseconds))
}

/// `DateTimeUTCOffset`: the UTC designator or a numeric offset, both
/// optional — but only ever after a time of day, which is why this is only
/// reached from the time-carrying branches.
fn scan_utc_offset_suffix(cursor: &mut Cursor, parsed: &mut Parsed) -> Option<()> {
    if cursor.eat_any(b"Zz").is_some() {
        parsed.utc_designator = true;
    } else if matches!(cursor.peek(), Some(b'+' | b'-')) {
        parsed.offset_nanoseconds = Some(scan_offset(cursor, true)?);
    }
    Some(())
}

/// `TimeZoneIdentifier`: either a minute-precision UTC offset, or an IANA
/// name whose *shape* is checked here — whether the name denotes a real
/// zone is a time-zone-resolution concern, not a grammar one
/// (`Temporal/Instant/from/argument-string.js` accepts
/// `[NotATimeZone]`).
fn is_valid_time_zone_identifier(body: &str) -> bool {
    if matches!(body.as_bytes().first(), Some(b'+' | b'-')) {
        let mut cursor = Cursor::new(body);
        return scan_offset(&mut cursor, false).is_some() && cursor.done();
    }
    !body.is_empty()
        && body.split('/').all(|component| {
            (1..=14).contains(&component.len())
                && component != "."
                && component != ".."
                && component
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'.' | b'_'))
                && component.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
                })
        })
}

fn is_valid_annotation_key(key: &str) -> bool {
    !key.is_empty()
        && key.bytes().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_lowercase() || byte == b'_'
            } else {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            }
        })
}

fn is_valid_annotation_value(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|component| {
            !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}

/// `TimeZoneAnnotation? Annotations?`: an optional leading time-zone
/// annotation followed by zero or more `[!]key=value` annotations.
///
/// Grammar notes, each taken from a pinned fixture rather than assumed:
/// the time-zone annotation must come first, so a second one anywhere later
/// is a syntax error (`Instant/from/argument-string-multiple-time-zone.js`);
/// an annotation key must be entirely lowercase regardless of the critical
/// (`!`) flag; an unrecognized key is ignored unless critical, in which case
/// this returns `Err`
/// (`PlainDateTime/from/argument-string-critical-unknown-annotation.js`); a
/// repeated `u-ca` annotation is ignored, but only when *no* `u-ca`
/// annotation is critical
/// (`PlainDateTime/from/argument-string-multiple-calendar.js`).
fn scan_annotations(cursor: &mut Cursor) -> Result<(Option<String>, Option<String>), ()> {
    let mut time_zone = None;
    if cursor.peek() == Some(b'[') {
        let body = cursor.peek_bracket().ok_or(())?;
        let name = body.strip_prefix('!').unwrap_or(body);
        if !name.contains('=') {
            if !is_valid_time_zone_identifier(name) {
                return Err(());
            }
            time_zone = Some(name.to_string());
            cursor.take_bracket().ok_or(())?;
        }
    }
    let mut calendar = None;
    let mut calendars = 0_usize;
    let mut critical_calendar = false;
    while cursor.peek() == Some(b'[') {
        let body = cursor.take_bracket().ok_or(())?;
        let (critical, body) = body
            .strip_prefix('!')
            .map_or((false, body), |rest| (true, rest));
        let (key, value) = body.split_once('=').ok_or(())?;
        if !is_valid_annotation_key(key) || !is_valid_annotation_value(value) {
            return Err(());
        }
        if key == "u-ca" {
            calendars += 1;
            critical_calendar |= critical;
            if calendar.is_none() {
                calendar = Some(value.to_string());
            }
        } else if critical {
            return Err(());
        }
    }
    if calendars > 1 && critical_calendar {
        return Err(());
    }
    Ok((time_zone, calendar))
}

/// Whether `value` matches Temporal's `TimeZoneIdentifier`: either a UTC
/// offset with at most minute precision, or an IANA-shaped name. Existence
/// of a named zone is deliberately not checked here.
pub(crate) fn is_time_zone_identifier(value: &str) -> bool {
    if value.starts_with(['+', '-']) {
        return parse_minute_precision_offset(value).is_some();
    }
    !value.is_empty()
        && value.split('/').all(|component| {
            (1..=14).contains(&component.len())
                && component
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'.'))
                && component.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-')
                })
        })
}

/// Parses a `±HH`, `±HH:MM` or `±HHMM` offset — the only offset forms a
/// `TimeZoneIdentifier` accepts — into signed nanoseconds. A seconds
/// component disqualifies the string even when it is `00`, because the
/// restriction is syntactic (Test262's `timezone-string-datetime.js` rejects
/// `-07:00:00` alongside `-07:00:01`).
pub(crate) fn parse_minute_precision_offset(value: &str) -> Option<i128> {
    if !value.starts_with(['+', '-']) {
        return None;
    }
    let (offset, rest) = parse_utc_offset_prefix(value)?;
    (rest.is_empty() && offset.minute_precision).then_some(offset.nanoseconds)
}

/// Splits exactly `count` leading ASCII digits off `source`.
fn split_digits(source: &str, count: usize) -> Option<(&str, &str)> {
    let bytes = source.as_bytes();
    if bytes.len() < count || !bytes[..count].iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(source.split_at(count))
}

/// Parses the date at the start of `source`, returning it with the
/// unconsumed remainder. Both the extended (`YYYY-MM-DD`) and basic
/// (`YYYYMMDD`) forms are accepted, with either a four-digit year or a
/// signed six-digit extended year — Test262's `Instant/from/argument-string.js`
/// exercises every combination, including mixing a basic date with an
/// extended time. `-000000` is never a valid extended year.
pub(crate) fn parse_iso_date_prefix(source: &str) -> Option<(CivilDate, &str)> {
    let (year, rest) = match source.as_bytes().first() {
        Some(sign @ (b'+' | b'-')) => {
            let negative = *sign == b'-';
            let (digits, rest) = split_digits(&source[1..], 6)?;
            let value: i32 = digits.parse().ok()?;
            if negative && value == 0 {
                return None;
            }
            (if negative { -value } else { value }, rest)
        }
        _ => {
            let (digits, rest) = split_digits(source, 4)?;
            (digits.parse().ok()?, rest)
        }
    };
    let (month, day, rest) = match rest.strip_prefix('-') {
        Some(rest) => {
            let (month, rest) = split_digits(rest, 2)?;
            let (day, rest) = split_digits(rest.strip_prefix('-')?, 2)?;
            (month, day, rest)
        }
        None => {
            let (month, rest) = split_digits(rest, 2)?;
            let (day, rest) = split_digits(rest, 2)?;
            (month, day, rest)
        }
    };
    let month: u8 = month.parse().ok()?;
    let day: u8 = day.parse().ok()?;
    ((-271_821..=275_760).contains(&year)
        && day >= 1
        && days_in_month(year, month).is_some_and(|last| day <= last))
    .then_some(((year, month, day), rest))
}

/// Parses a time-of-day at the start of `source`, returning it with the
/// unconsumed remainder (the UTC offset, designator or annotation that
/// follows). The time itself is delegated to [`parse_time_spec`], so the
/// extended/basic forms and the fraction rules stay defined in exactly one
/// place; a `60` second value is a leap second, clamped to `59` as
/// `ParseISODateTime` prescribes.
pub(crate) fn parse_iso_time_prefix(source: &str) -> Option<(CivilTime, &str)> {
    let end = source
        .find(['Z', 'z', '+', '-', '['])
        .unwrap_or(source.len());
    let (hour, minute, second, nanoseconds) = parse_time_spec(&source[..end])?;
    if second > 60 {
        return None;
    }
    Some((
        (
            hour,
            minute,
            second.min(59),
            (nanoseconds / 1_000_000) as u16,
            ((nanoseconds / 1_000) % 1_000) as u16,
            (nanoseconds % 1_000) as u16,
        ),
        &source[end..],
    ))
}

/// A parsed UTC offset: its exact value, plus whether it was written without
/// a seconds component.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UtcOffset {
    /// Signed nanoseconds east of UTC.
    pub(crate) nanoseconds: i128,
    /// Whether the written form had no seconds field (a `Z` designator
    /// counts, since it denotes exactly UTC).
    pub(crate) minute_precision: bool,
}

/// Parses a `Z`/`z` designator or a `±HH[[:]MM[[:]SS[.frac]]]` UTC offset at
/// the start of `source`. An offset shares the time-of-day grammar, so the
/// body goes through [`parse_time_spec`] too.
pub(crate) fn parse_utc_offset_prefix(source: &str) -> Option<(UtcOffset, &str)> {
    if let Some(rest) = source.strip_prefix(['Z', 'z']) {
        return Some((
            UtcOffset {
                nanoseconds: 0,
                minute_precision: true,
            },
            rest,
        ));
    }
    let negative = match source.as_bytes().first()? {
        b'+' => false,
        b'-' => true,
        _ => return None,
    };
    // An annotation, not a further offset field, is the only thing that may
    // follow an offset, so it is where the offset body ends.
    let end = source[1..]
        .find('[')
        .map_or(source.len(), |index| index + 1);
    let body = &source[1..end];
    let (hour, minute, second, nanoseconds) = parse_time_spec(body)?;
    if second > 59 {
        return None;
    }
    // Whether a seconds field was *written* (not merely non-zero) is what
    // decides whether the offset can serve as a `TimeZoneIdentifier`.
    let core = body.split(['.', ',']).next().unwrap_or(body);
    let has_second = if core.contains(':') {
        core.matches(':').count() == 2
    } else {
        core.len() == 6
    };
    let total = i128::from(hour) * 3_600_000_000_000
        + i128::from(minute) * 60_000_000_000
        + i128::from(second) * 1_000_000_000
        + i128::from(nanoseconds);
    Some((
        UtcOffset {
            nanoseconds: if negative { -total } else { total },
            minute_precision: !has_second,
        },
        &source[end..],
    ))
}

/// The pieces of a `TemporalInstantString`: a date, a time-of-day, and the
/// UTC offset that places them on the epoch timeline. Both the time and the
/// offset are mandatory for an `Instant` — a bare date, or a date with an
/// offset but no time, carries too little information (Test262's
/// `argument-string-date-with-utc-offset.js`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InstantParts {
    pub(crate) date: CivilDate,
    pub(crate) time: CivilTime,
    pub(crate) offset_nanoseconds: i128,
}

/// Parses a complete `TemporalInstantString`, rejecting trailing junk.
pub(crate) fn parse_instant(source: &str) -> Option<InstantParts> {
    let (date, rest) = parse_iso_date_prefix(source)?;
    let (time, rest) = parse_iso_time_prefix(rest.strip_prefix(['T', 't', ' '])?)?;
    let (offset, rest) = parse_utc_offset_prefix(rest)?;
    parse_annotation_suffix(rest).ok()?;
    Some(InstantParts {
        date,
        time,
        offset_nanoseconds: offset.nanoseconds,
    })
}

/// Resolves a Temporal `TimeZoneIdentifier`, or the time-zone information
/// carried by a full ISO date-time string, into the UTC offset (in
/// nanoseconds) that zone was actually observing at `epoch_nanoseconds`.
///
/// `Err(())` means the input is not a time zone at all (either malformed, or
/// syntactically shaped like an IANA name that names no real zone in the
/// pinned database). A named IANA zone's real historical offset comes from
/// Stage 1 Track E's `time_zone.rs`, which owns the actual transition data;
/// `UTC` and a fixed numeric offset are resolved directly here since they do
/// not depend on `epoch_nanoseconds` at all.
pub(crate) fn resolve_time_zone_offset(
    source: &str,
    epoch_nanoseconds: &BigInt,
) -> Result<i128, ()> {
    let identifier = if is_time_zone_identifier(source) {
        source.to_string()
    } else {
        // Not a bare identifier: the only other accepted form is a full ISO
        // date-time carrying either a time-zone annotation (which wins), a
        // `Z` designator, or a UTC offset.
        let (_, rest) = parse_iso_date_prefix(source).ok_or(())?;
        let rest = rest.strip_prefix(['T', 't', ' ']).ok_or(())?;
        let (_, rest) = parse_iso_time_prefix(rest).ok_or(())?;
        let (offset, rest) = match parse_utc_offset_prefix(rest) {
            Some((offset, rest)) => (Some(offset), rest),
            None => (None, rest),
        };
        match parse_annotation_suffix(rest)?.time_zone {
            Some(time_zone) => time_zone,
            // A `Z` designator means UTC; a written offset must be
            // minute-precision to serve as an identifier.
            None => {
                let offset = offset.ok_or(())?;
                if !offset.minute_precision {
                    return Err(());
                }
                return Ok(offset.nanoseconds);
            }
        }
    };
    if let Some(offset) = parse_minute_precision_offset(&identifier) {
        return Ok(offset);
    }
    if identifier.eq_ignore_ascii_case("UTC") {
        return Ok(0);
    }
    // A named IANA zone: `identifier` is already a bare `TimeZoneIdentifier`
    // at this point (either `source` itself, or the body of a winning
    // time-zone annotation), so this only ever takes the
    // `parse_bare_identifier` path inside `time_zone::parse_identifier` — it
    // re-validates the name against the same pinned database rather than
    // trusting the shape check above, and supplies the real offset lookup.
    let zone = super::time_zone::parse_identifier(&identifier).ok_or(())?;
    Ok(i128::from(zone.offset_nanoseconds_for(epoch_nanoseconds)))
}

/// `AnnotatedDateTime`: a required `ISODate`, an optional time of day (and,
/// only then, an optional UTC offset), and optional annotations. This is
/// the form `Temporal.PlainDate`, `PlainDateTime`, `Instant` and
/// `ZonedDateTime` strings are built from.
pub(crate) fn parse_date_time(source: &str) -> Option<Parsed> {
    let mut cursor = Cursor::new(source);
    let (year, month, day) = scan_date(&mut cursor)?;
    let mut parsed = Parsed {
        year,
        month,
        day,
        year_present: true,
        day_present: true,
        ..Parsed::default()
    };
    if cursor.eat_any(b"Tt ").is_some() {
        parsed.time = Some(scan_time(&mut cursor)?);
        scan_utc_offset_suffix(&mut cursor, &mut parsed)?;
    }
    let (time_zone, calendar) = scan_annotations(&mut cursor).ok()?;
    parsed.time_zone = time_zone;
    parsed.calendar = calendar;
    cursor.done().then_some(parsed)
}

/// The `DateYear ['-'] DateMonth` half of `TemporalYearMonthString` — the
/// form that carries no day at all.
fn parse_year_month_only(source: &str) -> Option<Parsed> {
    let mut cursor = Cursor::new(source);
    let year = scan_year(&mut cursor)?;
    cursor.eat(b'-');
    let month = cursor.digits(2)? as u8;
    if !(1..=12).contains(&month) {
        return None;
    }
    let (time_zone, calendar) = scan_annotations(&mut cursor).ok()?;
    cursor.done().then_some(Parsed {
        year,
        month,
        day: 1,
        year_present: true,
        day_present: false,
        time_zone,
        calendar,
        ..Parsed::default()
    })
}

/// `TemporalYearMonthString`: the day-less form, or any full
/// `AnnotatedDateTime`.
pub(crate) fn parse_year_month(source: &str) -> Option<Parsed> {
    parse_year_month_only(source).or_else(|| parse_date_time(source))
}

/// The `['--'] DateMonth ['-'] DateDay` half of `TemporalMonthDayString` —
/// the form that carries no year at all, so day validity is judged against
/// the ISO reference year 1972 (a leap year, which is why `02-29` is a
/// valid month-day).
fn parse_month_day_only(source: &str) -> Option<Parsed> {
    let mut cursor = Cursor::new(source);
    if cursor.eat(b'-') && !cursor.eat(b'-') {
        return None;
    }
    let month = cursor.digits(2)? as u8;
    cursor.eat(b'-');
    let day = cursor.digits(2)? as u8;
    if !is_valid_date(1972, month, day) {
        return None;
    }
    let (time_zone, calendar) = scan_annotations(&mut cursor).ok()?;
    cursor.done().then_some(Parsed {
        year: 1972,
        month,
        day,
        year_present: false,
        day_present: true,
        time_zone,
        calendar,
        ..Parsed::default()
    })
}

/// `TemporalMonthDayString`: the year-less form, or any full
/// `AnnotatedDateTime`.
pub(crate) fn parse_month_day(source: &str) -> Option<Parsed> {
    parse_month_day_only(source).or_else(|| parse_date_time(source))
}

/// Whether a bare (undesignated) time string would also read as a
/// year-month or month-day string. Temporal resolves that ambiguity by
/// requiring the `T` designator rather than by preferring one reading:
/// `1130` is a syntax error as a time, while `1314` (no such month) is
/// 13:14 — see `TemporalHelpers.ISO.plainTimeStringsAmbiguous()`.
fn is_ambiguous_with_a_date(source: &str) -> bool {
    parse_year_month_only(source).is_some() || parse_month_day_only(source).is_some()
}

/// `AnnotatedTime`: a time of day, optionally introduced by the `T`/`t`
/// designator, with an optional UTC offset and annotations.
fn parse_time_only(source: &str) -> Option<Parsed> {
    let mut cursor = Cursor::new(source);
    let designated = cursor.eat_any(b"Tt").is_some();
    let mut parsed = Parsed {
        time: Some(scan_time(&mut cursor)?),
        ..Parsed::default()
    };
    scan_utc_offset_suffix(&mut cursor, &mut parsed)?;
    let (time_zone, calendar) = scan_annotations(&mut cursor).ok()?;
    parsed.time_zone = time_zone;
    parsed.calendar = calendar;
    if !cursor.done() || (!designated && is_ambiguous_with_a_date(source)) {
        return None;
    }
    Some(parsed)
}

/// `TemporalTimeString`: a time-only string, or a full
/// `AnnotatedDateTime` that actually carries a time (a date alone never
/// implies midnight —
/// `PlainTime/from/argument-string-no-implicit-midnight.js`).
pub(crate) fn parse_time(source: &str) -> Option<Parsed> {
    if let Some(parsed) = parse_time_only(source) {
        return Some(parsed);
    }
    let parsed = parse_date_time(source)?;
    parsed.time.is_some().then_some(parsed)
}

/// `TemporalTimeString` reduced to just its time of day, which is all a
/// `Temporal.PlainTime` retains. A UTC designator asserts an exact instant,
/// so it is not valid on a wall-clock time
/// (`PlainTime/from/argument-string-with-utc-designator.js`).
pub(crate) fn parse_plain_time(source: &str) -> Option<Time> {
    let parsed = parse_time(source)?;
    if parsed.utc_designator {
        return None;
    }
    parsed.time
}

/// A whole-string, minute-precision UTC offset, as a fixed-offset time-zone
/// identifier is spelled. Returns nanoseconds east of UTC.
pub(crate) fn parse_offset_identifier_nanoseconds(source: &str) -> Option<i64> {
    let mut cursor = Cursor::new(source);
    let offset = scan_offset(&mut cursor, false)?;
    cursor.done().then_some(offset)
}

/// Parses the ISO duration strings accepted by `Intl.DurationFormat` and
/// `Temporal.Duration` through Temporal's duration-string grammar. The host
/// service receives a typed, validated ECMA-402 record, so neither this
/// parser nor a Temporal object can trigger observable duration-field
/// accessors while formatting.
///
/// Temporal's grammar, as this implements it (each rule checked against a
/// named fixture in the pinned Test262 corpus rather than assumed — see
/// `built-ins/Temporal/Duration/from/argument-string*.js`):
///
/// - the designators and the `P`/`T` markers are case-insensitive, and an
///   optional leading sign may be `+`, `-` or U+2212 MINUS SIGN;
/// - components must appear in descending order and at most once each;
/// - a decimal fraction (`.` or `,`, one to nine digits) may appear on *any*
///   time component, not only on seconds, but then nothing smaller may
///   follow. `PT0.5H` is 30 minutes, not an error. Its value is carried down
///   through the smaller fields exactly: a fraction of an hour, minute or
///   second is always a whole number of nanoseconds.
pub(crate) fn parse_duration_record(source: &str) -> Option<blueice_ecma402::DurationRecord> {
    let mut characters = source.chars().peekable();
    let sign = match characters.peek() {
        Some('+') => {
            characters.next();
            1_i128
        }
        Some('-' | '\u{2212}') => {
            characters.next();
            -1_i128
        }
        _ => 1_i128,
    };
    matches!(characters.next(), Some('P' | 'p')).then_some(())?;
    let mut values = [0_i128; 10];
    let mut in_time = false;
    let mut saw_component = false;
    let mut saw_time_component = false;
    let mut previous: Option<usize> = None;
    let mut fraction_seen = false;
    while let Some(&character) = characters.peek() {
        // A fractional component must be the last one in the string.
        if fraction_seen {
            return None;
        }
        if matches!(character, 'T' | 't') {
            if in_time {
                return None;
            }
            characters.next();
            in_time = true;
            previous = None;
            continue;
        }
        let mut number = String::new();
        while characters
            .peek()
            .is_some_and(|character| character.is_ascii_digit())
        {
            number.push(characters.next()?);
        }
        if number.is_empty() {
            return None;
        }
        let mut fraction = None;
        if matches!(characters.peek(), Some('.' | ',')) {
            characters.next();
            let mut digits = String::new();
            while characters
                .peek()
                .is_some_and(|character| character.is_ascii_digit())
            {
                digits.push(characters.next()?);
            }
            if digits.is_empty() || digits.len() > 9 {
                return None;
            }
            fraction = Some(digits);
        }
        let index = match (in_time, characters.next()?.to_ascii_uppercase()) {
            (false, 'Y') => 0,
            (false, 'M') => 1,
            (false, 'W') => 2,
            (false, 'D') => 3,
            (true, 'H') => 4,
            (true, 'M') => 5,
            (true, 'S') => 6,
            _ => return None,
        };
        if previous.is_some_and(|previous| index <= previous) {
            return None;
        }
        previous = Some(index);
        saw_component = true;
        saw_time_component |= in_time;
        values[index] = number.parse::<i128>().ok()?;
        if let Some(digits) = fraction {
            // A fraction of an hour, minute or second is an exact whole
            // number of nanoseconds, so this carries down without rounding.
            let unit_nanoseconds: i128 = match index {
                4 => 3_600_000_000_000,
                5 => 60_000_000_000,
                6 => 1_000_000_000,
                _ => return None,
            };
            let scale = 10_i128.pow(digits.len() as u32);
            let mut remaining = digits.parse::<i128>().ok()? * unit_nanoseconds / scale;
            for (slot, unit) in [
                (5_usize, 60_000_000_000_i128),
                (6, 1_000_000_000),
                (7, 1_000_000),
                (8, 1_000),
                (9, 1),
            ] {
                if slot <= index {
                    continue;
                }
                values[slot] = remaining / unit;
                remaining %= unit;
            }
            fraction_seen = true;
        }
    }
    (saw_component && (!in_time || saw_time_component)).then_some(())?;
    blueice_ecma402::DurationRecord::try_new(
        sign * values[0],
        sign * values[1],
        sign * values[2],
        sign * values[3],
        sign * values[4],
        sign * values[5],
        sign * values[6],
        sign * values[7],
        sign * values[8],
        sign * values[9],
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`parse_annotation_suffix`], keeping only the resolved calendar.
    fn parse_annotations(source: &str) -> Result<Option<String>, ()> {
        parse_annotation_suffix(source).map(|annotations| annotations.calendar)
    }

    fn date(source: &str) -> Option<(i32, u8, u8)> {
        parse_date_time(source).map(|parsed| (parsed.year, parsed.month, parsed.day))
    }

    fn time_of(source: &str) -> Option<Time> {
        parse_time(source).and_then(|parsed| parsed.time)
    }

    fn offset(source: &str) -> Option<i64> {
        parse_date_time(source).and_then(|parsed| parsed.offset_nanoseconds)
    }

    #[test]
    fn parses_extended_and_basic_calendar_dates() {
        // Temporal/PlainDate/from/argument-string.js.
        assert_eq!(date("2000-05-02"), Some((2000, 5, 2)));
        assert_eq!(date("+002020-06-01"), Some((2020, 6, 1)));
        assert_eq!(date("-010583-06-30"), Some((-10583, 6, 30)));
        assert_eq!(date("19761118"), Some((1976, 11, 18)));
        assert_eq!(date("+0019761118"), Some((1976, 11, 18)));
        assert_eq!(date("2000-05-02T15:23"), Some((2000, 5, 2)));
        assert_eq!(date("-999999-01-01"), Some((-999_999, 1, 1)));
    }

    #[test]
    fn rejects_dates_that_break_the_grammar() {
        // Temporal/PlainDate/from/argument-string-invalid.js.
        for source in [
            "",
            "invalid iso8601",
            "2020-01-00",
            "2020-01-32",
            "2020-02-30",
            "2021-02-29",
            "2020-00-01",
            "2020-13-01",
            "02020-01-01",
            "2020-001-01",
            "2020-01-001",
            "+0002020-01-01",
            "2020-W01-1",
            "2020-001",
            "2020-01",
            "+002020-01",
            "01-01",
            "2020-0101",
            "202001-01",
            "1976-11-18junk",
            // Temporal/PlainDate/from/year-zero.js: a signed zero year.
            "-000000-10-31",
            "-000000-10-31T00:45",
            // Temporal/PlainDate/from/argument-string-minus-sign.js: the
            // Unicode minus sign U+2212 is not an ASCII sign.
            "\u{2212}009999-11-18T15:23:30.12",
            // Neither year width may borrow the other's: a six-digit year
            // needs its sign, and a four-digit one must not carry one.
            "002020-06-01",
            "+2020-06-01",
        ] {
            assert_eq!(date(source), None, "{source}");
        }
        // `+000000` is a legal extended year; only the *negative* zero is
        // prohibited.
        assert_eq!(date("+000000-12-07"), Some((0, 12, 7)));
        assert!(is_leap_year(2020) && !is_leap_year(2021));
    }

    #[test]
    fn parses_time_of_day_in_both_forms_and_every_precision() {
        // Temporal/PlainTime/from/argument-string.js.
        assert_eq!(time_of("15"), Some((15, 0, 0, 0, 0, 0)));
        assert_eq!(time_of("15:23"), Some((15, 23, 0, 0, 0, 0)));
        assert_eq!(time_of("152330"), Some((15, 23, 30, 0, 0, 0)));
        assert_eq!(time_of("152330.1"), Some((15, 23, 30, 100, 0, 0)));
        assert_eq!(time_of("T00:30"), Some((0, 30, 0, 0, 0, 0)));
        assert_eq!(time_of("t003000.000000000"), Some((0, 30, 0, 0, 0, 0)));
        assert_eq!(
            time_of("15:23:30.123456789"),
            Some((15, 23, 30, 123, 456, 789))
        );
        // Temporal/Instant/from/argument-string.js: `,` is also a decimal
        // separator.
        assert_eq!(
            time_of("1976-11-18T15:23:30,12"),
            Some((15, 23, 30, 120, 0, 0))
        );
        // A space is a date/time separator, per
        // Temporal/PlainDate/from/argument-string-time-separators.js.
        assert_eq!(time_of("2000-05-02 15:23"), Some((15, 23, 0, 0, 0, 0)));
        assert_eq!(time_of("2000-05-02t15:23"), Some((15, 23, 0, 0, 0, 0)));
        // Temporal/PlainDate/from/argument-leap-second.js: a leap second
        // parses, clamped to :59.
        assert_eq!(time_of("2016-12-31T23:59:60"), Some((23, 59, 59, 0, 0, 0)));
    }

    #[test]
    fn rejects_times_that_break_the_grammar() {
        // Temporal/PlainTime/from/argument-string-invalid.js and
        // Temporal/PlainDate/from/argument-string-{too-many-decimals,
        // invalid}.js, no-fractional-minutes-hours.js.
        for source in [
            "24:00",
            "15:60",
            "25:00:00",
            "01:60:00",
            "01:60:61",
            "001",
            "01:001",
            "01:01:001",
            "00:00:00.1234567891",
            "0000:00",
            "00:0000",
            "00:00:00+00:0000",
            "00:00:00+0000:00",
            "05:07.123",
            "12.5",
            "15:23:30.",
            "2019-10-01",
            "Z",
            "1976-11-18T15:23:30.12\u{2212}02:00",
            "",
            "00:00:00:00",
            "00:",
            "0:0",
            "1",
            "00:61",
            "00:00:61",
            "15:23:30.100junk",
        ] {
            assert_eq!(time_of(source), None, "{source}");
        }
        // A `PlainTime` additionally rejects the UTC designator, which a
        // wall-clock time cannot represent.
        assert_eq!(parse_plain_time("09:00:00Z"), None);
        assert_eq!(parse_plain_time("2019-10-01T09:00:00Z[UTC]"), None);
        assert_eq!(parse_plain_time("T"), None);
        assert_eq!(
            parse_plain_time("1976-11-18 12:34:56.987654321"),
            Some((12, 34, 56, 987, 654, 321))
        );
        assert_eq!(
            parse_plain_time("12:34:56.987654321+00:00:00,0"),
            Some((12, 34, 56, 987, 654, 321))
        );
    }

    #[test]
    fn resolves_the_first_calendar_annotation_and_ignores_later_ones() {
        assert_eq!(
            parse_annotations("[u-ca=hebrew]"),
            Ok(Some("hebrew".to_string()))
        );
        assert_eq!(
            parse_annotations("[u-ca=hebrew][u-ca=discord]"),
            Ok(Some("hebrew".to_string()))
        );
        assert_eq!(
            parse_annotations("[UTC][u-ca=hebrew]"),
            Ok(Some("hebrew".to_string()))
        );
        assert_eq!(parse_annotations("[foo=bar]"), Ok(None));
        assert_eq!(parse_annotations(""), Ok(None));
    }

    #[test]
    fn rejects_invalid_or_critical_unknown_annotations() {
        assert_eq!(parse_annotations("[!foo=bar]"), Err(()));
        assert_eq!(parse_annotations("[FOO=bar]"), Err(()));
        assert_eq!(parse_annotations("[u-CA=iso8601]"), Err(()));
    }

    fn duration(source: &str) -> Option<[i128; 10]> {
        let record = parse_duration_record(source)?;
        Some([
            record.years,
            record.months,
            record.weeks,
            record.days,
            record.hours,
            record.minutes,
            record.seconds,
            record.milliseconds,
            record.microseconds,
            record.nanoseconds,
        ])
    }

    #[test]
    fn rejects_more_than_one_calendar_annotation_when_any_is_critical() {
        // Test262's PlainTime/prototype/until/argument-string-multiple-calendar.js
        // (and the identically-shaped PlainDate/PlainDateTime fixtures): a
        // repeated `u-ca` annotation is ignored, but becomes a syntax error
        // as soon as any of them carries the critical flag.
        assert_eq!(
            parse_annotations("[u-ca=iso8601][u-ca=discord]"),
            Ok(Some("iso8601".to_string()))
        );
        assert_eq!(parse_annotations("[u-ca=iso8601][!u-ca=iso8601]"), Err(()));
        assert_eq!(parse_annotations("[!u-ca=iso8601][u-ca=iso8601]"), Err(()));
        assert_eq!(
            parse_annotations("[UTC][u-ca=iso8601][!u-ca=iso8601]"),
            Err(())
        );
        // A single critical calendar annotation stays valid.
        assert_eq!(
            parse_annotations("[!u-ca=iso8601]"),
            Ok(Some("iso8601".to_string()))
        );
    }

    #[test]
    fn parses_the_plain_time_string_grammar() {
        // Test262's PlainTime/prototype/until/argument-string-time-separators.js
        // and .../argument-string-calendar-annotation.js.
        for source in [
            "12:34:56.987654321",
            "T12:34:56.987654321",
            "t12:34:56.987654321",
            "1976-11-18T12:34:56.987654321",
            "1976-11-18t12:34:56.987654321",
            "1976-11-18 12:34:56.987654321",
            "12:34:56.987654321[u-ca=iso8601]",
            "12:34:56.987654321[UTC][u-ca=iso8601]",
            "12:34:56.987654321[!u-ca=unknown]",
            "12:34:56.987654321+00:00",
            "12:34:56.987654321-02:30[America/St_Johns]",
            "12:34:56.987654321+00:00:00,0",
            "1976-11-18T12:34:56.987654321+00:00[UTC]",
        ] {
            assert_eq!(
                parse_plain_time(source),
                Some((12, 34, 56, 987, 654, 321)),
                "{source}"
            );
        }
        assert_eq!(parse_plain_time("15:23"), Some((15, 23, 0, 0, 0, 0)));
        // A leap second is constrained to :59, per ParseISODateTime.
        assert_eq!(parse_plain_time("23:59:60"), Some((23, 59, 59, 0, 0, 0)));
        assert_eq!(
            parse_plain_time("23:59:60.170"),
            Some((23, 59, 59, 170, 0, 0))
        );
    }

    #[test]
    fn requires_a_time_designator_only_where_a_date_reading_is_ambiguous() {
        // TemporalHelpers.ISO.plainTimeStringsAmbiguous(), via
        // PlainTime/from/argument-string-time-designator-required-for-disambiguation.js.
        for source in [
            "2021-12",
            "2021-12[-12:00]",
            "1214",
            "0229",
            "1130",
            "12-14",
            "12-14[-14:00]",
            "202112",
            "202112[UTC]",
            "2021-12[u-ca=iso8601]",
            "1130[u-ca=iso8601]",
        ] {
            assert_eq!(time_of(source), None, "ambiguous: {source}");
            assert!(
                time_of(&format!("T{source}")).is_some(),
                "designated: T{source}"
            );
        }
        // A space is only ever a date/time separator, never a substitute for
        // the time designator.
        assert_eq!(time_of(" 1130"), None);
        assert_eq!(time_of(" 15:23"), None);
        // TemporalHelpers.ISO.plainTimeStringsUnambiguous(): no month 13, no
        // 32nd day, no February 30th, no zeroth month.
        for (source, expected) in [
            ("2021-13", (20, 21, 0, 0, 0, 0)),
            ("202113", (20, 21, 13, 0, 0, 0)),
            ("0000-00", (0, 0, 0, 0, 0, 0)),
            ("000000", (0, 0, 0, 0, 0, 0)),
            ("1314", (13, 14, 0, 0, 0, 0)),
            ("13-14", (13, 0, 0, 0, 0, 0)),
            ("1232", (12, 32, 0, 0, 0, 0)),
            ("0230", (2, 30, 0, 0, 0, 0)),
            ("0631", (6, 31, 0, 0, 0, 0)),
            ("0000", (0, 0, 0, 0, 0, 0)),
            ("00-00", (0, 0, 0, 0, 0, 0)),
        ] {
            assert_eq!(time_of(source), Some(expected), "unambiguous: {source}");
        }
    }

    #[test]
    fn parses_duration_strings_including_a_fraction_on_any_time_component() {
        // Every case here is taken from Test262's
        // built-ins/Temporal/Duration/from/argument-string.js.
        assert_eq!(duration("P1D"), Some([0, 0, 0, 1, 0, 0, 0, 0, 0, 0]));
        assert_eq!(
            duration("p1y1m1dt1h1m1s"),
            Some([1, 1, 0, 1, 1, 1, 1, 0, 0, 0]),
            "designators are case-insensitive"
        );
        assert_eq!(
            duration("P1Y1M1W1DT1H1M1.1S"),
            Some([1, 1, 1, 1, 1, 1, 1, 100, 0, 0])
        );
        assert_eq!(
            duration("P1Y1M1W1DT1H1M1.1234567S"),
            Some([1, 1, 1, 1, 1, 1, 1, 123, 456, 700])
        );
        assert_eq!(
            duration("P1Y1M1W1DT1H1M1,12S"),
            Some([1, 1, 1, 1, 1, 1, 1, 120, 0, 0]),
            "a comma is a decimal separator"
        );
        assert_eq!(
            duration("P1DT0.5M"),
            Some([0, 0, 0, 1, 0, 0, 30, 0, 0, 0]),
            "half a minute is thirty seconds"
        );
        assert_eq!(
            duration("P1DT0,5H"),
            Some([0, 0, 0, 1, 0, 30, 0, 0, 0, 0]),
            "half an hour is thirty minutes"
        );
        assert_eq!(
            duration("P1DT2H30.5M"),
            Some([0, 0, 0, 1, 2, 30, 30, 0, 0, 0]),
            "a fraction is valid on the last component present"
        );
        assert_eq!(
            duration("PT0.999999999H"),
            Some([0, 0, 0, 0, 0, 59, 59, 999, 996, 400]),
            "nine fractional hour digits are 3,599,999,996,400 ns, carried down exactly"
        );
        assert_eq!(duration("-P1D"), Some([0, 0, 0, -1, 0, 0, 0, 0, 0, 0]));
        assert_eq!(
            duration("\u{2212}P1D"),
            Some([0, 0, 0, -1, 0, 0, 0, 0, 0, 0]),
            "U+2212 MINUS SIGN is a sign"
        );
        assert_eq!(duration("+P1D"), Some([0, 0, 0, 1, 0, 0, 0, 0, 0, 0]));
        assert_eq!(duration("PT100M"), Some([0, 0, 0, 0, 0, 100, 0, 0, 0, 0]));
    }

    #[test]
    fn rejects_duration_strings_outside_the_grammar() {
        assert_eq!(duration("not-a-duration"), None);
        assert_eq!(duration("P"), None, "at least one component is required");
        assert_eq!(duration("PT"), None, "a time designator needs a component");
        assert_eq!(duration("P1DT"), None);
        assert_eq!(duration("P1.5D"), None, "a date component has no fraction");
        assert_eq!(
            duration("PT0.5H30M"),
            None,
            "nothing smaller may follow a fraction"
        );
        assert_eq!(
            duration("P1M1Y"),
            None,
            "components are in descending order"
        );
        assert_eq!(duration("P1D1D"), None, "a component appears at most once");
        assert_eq!(duration("P1H"), None, "hours require the time designator");
        assert_eq!(duration("PT1D"), None, "days precede the time designator");
        assert_eq!(duration("PT1.1234567890S"), None, "at most nine digits");
        assert_eq!(duration("PT1.S"), None, "a fraction needs a digit");
        assert_eq!(duration("PT1HT1M"), None, "only one time designator");
        assert_eq!(duration("PD"), None, "a designator needs a number");
        assert_eq!(duration("P-1D"), None, "a component may not carry a sign");
    }

    #[test]
    fn parses_duration_strings_with_a_fraction_on_the_last_unit_only() {
        assert!(parse_duration_record("P1Y2M3W4DT5H6M7.008009010S").is_some());
        // A fraction on the last present unit is valid, and cascades into
        // the units below it: `PT30.5M` is 30 minutes and 30 seconds.
        // (Test262 evidence that this *must* parse:
        // `built-ins/Temporal/Instant/prototype/add/argument-string-negative-fractional-units.js`
        // adds `"-PT1440.567890123M"` to an Instant.)
        let record = parse_duration_record("P1DT2H30.5M").expect("a fractional minute is valid");
        assert_eq!(record.minutes, 30);
        assert_eq!(record.seconds, 30);
        // A fraction anywhere but on the last unit is still a syntax error.
        assert!(parse_duration_record("PT30.5M10S").is_none());
        assert!(parse_duration_record("P1.5DT1H").is_none());
        assert!(parse_duration_record("not-a-duration").is_none());
        assert!(parse_duration_record("P1DT").is_none());
    }

    #[test]
    fn cascades_a_fractional_hour_into_every_lower_unit() {
        // Test262's `add/argument-string-fractional-units-rounding-mode.js`:
        // `PT1.03125H` is exactly 3,712.5 seconds.
        let record = parse_duration_record("PT1.03125H").expect("a fractional hour is valid");
        assert_eq!(record.hours, 1);
        assert_eq!(record.minutes, 1);
        assert_eq!(record.seconds, 52);
        assert_eq!(record.milliseconds, 500);
        // `add/argument-string-negative-fractional-units.js`'s exact values.
        let record = parse_duration_record("-PT24.567890123H").expect("a fractional hour is valid");
        assert_eq!(
            (
                record.hours,
                record.minutes,
                record.seconds,
                record.milliseconds,
                record.microseconds,
                record.nanoseconds
            ),
            (-24, -34, -4, -404, -442, -800)
        );
    }

    #[test]
    fn parses_offsets_including_zulu_and_bracket_termination() {
        assert_eq!(
            parse_utc_offset_prefix("Z").map(|(offset, rest)| (offset.nanoseconds, rest)),
            Some((0, ""))
        );
        assert_eq!(
            parse_utc_offset_prefix("+01:00").map(|(offset, rest)| (offset.nanoseconds, rest)),
            Some((3_600_000_000_000, ""))
        );
        assert_eq!(
            parse_utc_offset_prefix("-0130").map(|(offset, rest)| (offset.nanoseconds, rest)),
            Some((-5_400_000_000_000, ""))
        );
        assert_eq!(
            parse_utc_offset_prefix("-08:00[America/Vancouver]")
                .map(|(offset, rest)| (offset.nanoseconds, rest)),
            Some((-28_800_000_000_000, "[America/Vancouver]"))
        );
        assert_eq!(parse_utc_offset_prefix("+25:00"), None);
        assert_eq!(parse_utc_offset_prefix("01:00"), None);
    }

    #[test]
    fn records_whether_an_offset_was_written_with_minute_precision() {
        for (source, minute_precision) in [
            ("Z", true),
            ("+00", true),
            ("+00:00", true),
            ("+0000", true),
            ("+00:00:00", false),
            ("+000000", false),
            ("+00:00:00.5", false),
        ] {
            let (offset, rest) =
                parse_utc_offset_prefix(source).unwrap_or_else(|| panic!("{source} parses"));
            assert!(rest.is_empty(), "{source}");
            assert_eq!(offset.minute_precision, minute_precision, "{source}");
        }
    }

    #[test]
    fn parses_the_instant_grammar_including_leap_seconds_and_variant_separators() {
        assert_eq!(
            parse_instant("1970-01-01T00:00Z"),
            Some(InstantParts {
                date: (1970, 1, 1),
                time: (0, 0, 0, 0, 0, 0),
                offset_nanoseconds: 0,
            })
        );
        for separator in ['T', 't', ' '] {
            assert!(
                parse_instant(&format!("1970-01-01{separator}00:00Z")).is_some(),
                "{separator}"
            );
        }
        // Leap seconds clamp to :59 rather than being rejected.
        assert_eq!(
            parse_instant("2016-12-31T23:59:60Z").map(|parts| parts.time),
            Some((23, 59, 59, 0, 0, 0))
        );
        // Sub-minute offsets are exact in the offset position.
        assert_eq!(
            parse_instant("1970-01-01T00:19:32.37+00:19:32.37")
                .map(|parts| (parts.time, parts.offset_nanoseconds)),
            Some(((0, 19, 32, 370, 0, 0), 1_172_370_000_000))
        );
        // A calendar annotation is accepted and never resolved.
        assert!(parse_instant("1970-01-01T00:00Z[u-ca=discord]").is_some());
        assert!(parse_instant("1970-01-01T00Z[Europe/Vienna]").is_some());
    }

    #[test]
    fn rejects_instant_strings_the_grammar_does_not_allow() {
        for source in [
            "",
            "invalid iso8601",
            // A bare date, or a date without a time, is not an instant.
            "2020-01-01",
            "2020-01-01T00:00:00",
            "2022-09-15Z",
            "2022-09-15+00:00[UTC]",
            "2020-01-01TZ",
            // Out-of-range or malformed components.
            "2020-01-00T00:00Z",
            "2020-02-30T00:00Z",
            "2020-13-01T00:00Z",
            "2020-01-01T25:00:00Z",
            "2020-01-01T01:60:00Z",
            "2020-01-01T00:00-24:00",
            // Trailing junk.
            "2020-01-01T00:00Zjunk",
            "2020-01-01T00:00:00+00:00[UTC][u-ca=iso8601]junk",
            // Unsupported year/component widths.
            "02020-01-01T00:00Z",
            "+0002020-01-01T00:00Z",
            "2020-001-01T00:00Z",
            "2020-01-001T00:00Z",
            "2020-01-01T001Z",
            "2020-01-01T01:001Z",
            "2020-W01-1T00:00Z",
            // Negative zero is never a valid extended year.
            "-000000-03-30T00:45Z",
            // More than nine fractional digits.
            "1970-01-01T00:00:00.1234567891Z",
            "1970-01-01T00+00:00:00.1234567890",
            // A sub-minute offset cannot be a time-zone annotation.
            "2021-08-19T17:30-07:00:01[-07:00:01]",
            "2021-08-19T17:30-07:00:00[-070000]",
            // More than one calendar annotation, any of them critical.
            "1970-01-01T00:00Z[u-ca=iso8601][!u-ca=iso8601]",
            "1970-01-01T00:00Z[!u-ca=iso8601][u-ca=iso8601]",
        ] {
            assert_eq!(parse_instant(source), None, "{source:?}");
        }
    }

    #[test]
    fn resolves_utc_and_fixed_offset_time_zones_regardless_of_the_instant() {
        let epoch = BigInt::from(0);
        for (source, expected) in [
            ("UTC", Ok(0)),
            ("utc", Ok(0)),
            ("+01:00", Ok(3_600_000_000_000)),
            ("-01:30", Ok(-5_400_000_000_000)),
            ("2021-08-19T17:30Z", Ok(0)),
            ("2021-08-19T17:30-07:00", Ok(-25_200_000_000_000)),
            ("2021-08-19T17:30-07:00[UTC]", Ok(0)),
            (
                "2021-08-19T17:30:45.123456789-12:12[+01:46]",
                Ok(6_360_000_000_000),
            ),
            ("2016-12-31T23:59:60+00:00[UTC]", Ok(0)),
            // Not a real time zone at all.
            ("", Err(())),
            ("2021-08-19T17:30", Err(())),
            ("2021-08-19T17:30-07:00:01", Err(())),
            ("2021-08-19T17:30-07:00:00", Err(())),
            ("2021-08-19T17:30:45.123456789+23:59[+23:59:60]", Err(())),
            // Syntactically zone-shaped, but not a real IANA name.
            ("Mars/Olympus_Mons", Err(())),
        ] {
            assert_eq!(
                resolve_time_zone_offset(source, &epoch),
                expected,
                "{source:?}"
            );
        }
    }

    #[test]
    fn resolves_real_historical_offsets_for_named_iana_zones() {
        // Cases taken directly from the pinned Test262 corpus's
        // intl402/Temporal/Instant/prototype/toString/timezone-offset.js,
        // all at the epoch instant `new Temporal.Instant(0n)` uses.
        let epoch = BigInt::from(0);
        assert_eq!(
            resolve_time_zone_offset("Europe/Berlin", &epoch),
            Ok(3_600_000_000_000)
        );
        assert_eq!(
            resolve_time_zone_offset("America/New_York", &epoch),
            Ok(-5 * 3_600_000_000_000)
        );
        // A sub-minute historical offset: Monrovia was UTC-00:44:30 before
        // 1972 (the fixture's own expected display string,
        // "1969-12-31T23:15:30-00:45", rounds this to the minute for
        // `toString`'s offset field, but the underlying instant's real
        // offset — what this function resolves — is the exact -00:44:30).
        assert_eq!(
            resolve_time_zone_offset("Africa/Monrovia", &epoch),
            Ok(-(44 * 60_000_000_000_i128) - 30_000_000_000)
        );
        // A different instant in the same named zone resolves a different
        // (real, historical) offset — summer vs. winter New York.
        let summer = BigInt::from(1_720_480_004_i64) * 1_000_000_000_u32;
        assert_eq!(
            resolve_time_zone_offset("America/New_York", &summer),
            Ok(-4 * 3_600_000_000_000)
        );
        // A time-zone annotation's IANA name wins over the string's own
        // offset, and is resolved at the receiver's instant, not a fixed
        // offset taken from the string.
        assert_eq!(
            resolve_time_zone_offset("2021-08-19T17:30-07:00[America/Vancouver]", &epoch),
            resolve_time_zone_offset("America/Vancouver", &epoch)
        );
    }

    #[test]
    fn parses_year_month_and_month_day_short_forms() {
        // TemporalHelpers.ISO.plainYearMonthStringsValid().
        for source in ["1976-11", "197611", "+00197611", "1976-11-10"] {
            let parsed = parse_year_month(source).expect(source);
            assert_eq!((parsed.year, parsed.month), (1976, 11), "{source}");
        }
        assert!(!parse_year_month("1976-11").unwrap().day_present);
        assert!(parse_year_month("1976-11-10").unwrap().day_present);
        assert_eq!(parse_year_month("-009999-11").unwrap().year, -9999);
        // TemporalHelpers.ISO.plainYearMonthStringsInvalid().
        for source in ["2020-13", "1976-11[U-CA=iso8601]", "1976-11[FOO=bar]"] {
            assert_eq!(parse_year_month(source), None, "{source}");
        }
        // TemporalHelpers.ISO.plainMonthDayStringsValid().
        for source in ["10-01", "1001", "--10-01", "--1001", "1965-10-01"] {
            let parsed = parse_month_day(source).expect(source);
            assert_eq!((parsed.month, parsed.day), (10, 1), "{source}");
        }
        assert!(!parse_month_day("--10-01").unwrap().year_present);
        assert!(parse_month_day("1965-10-01").unwrap().year_present);
        // February 29th is a valid month-day: the reference year is a leap
        // year.
        assert!(parse_month_day("02-29").is_some());
        assert_eq!(parse_month_day("02-30"), None);
        // TemporalHelpers.ISO.plainMonthDayStringsInvalid().
        assert_eq!(parse_month_day("11-18junk"), None);
        // Out-of-range years stay a caller's concern, not the grammar's.
        assert_eq!(parse_month_day("-999999-10-01").unwrap().year, -999_999);
    }

    #[test]
    fn parses_utc_designators_and_offsets_down_to_nanoseconds() {
        // Temporal/Instant/from/argument-string.js and
        // instant-string-sub-minute-offset.js.
        assert!(parse_date_time("1976-11-18T15:23z").unwrap().utc_designator);
        assert_eq!(offset("1976-11-18T15:23:30+00"), Some(0));
        assert_eq!(
            offset("1976-11-18T15:23:30-02:00"),
            Some(-7_200_000_000_000)
        );
        assert_eq!(offset("1976-11-18T15:23:30+0000"), Some(0));
        assert_eq!(
            offset("1970-01-01T00:19:32.37+00:19:32.37"),
            Some(1_172_370_000_000)
        );
        assert_eq!(
            offset("1976-11-18T15:23:30.123456789-00:00:00.000000001"),
            Some(-1)
        );
        assert_eq!(offset("2000-05-02T00+000000,0"), Some(0));
        // Out-of-range and mis-separated offsets.
        for source in [
            "2020-01-01T00:00-24:00",
            "2020-01-01T00:00+24:00",
            "2025-01-01T00:00:00+00:0000",
            "2025-01-01T00:00:00+0000:00",
        ] {
            assert_eq!(parse_date_time(source), None, "{source}");
        }
        // A UTC offset is only ever legal after a time of day:
        // PlainDate/from/argument-string-date-with-utc-offset.js.
        for source in ["2022-09-15+00:00", "2022-09-15-02:30", "2022-09-15Z"] {
            assert_eq!(parse_date_time(source), None, "{source}");
        }
        // A fixed-offset time-zone identifier, read on its own.
        assert_eq!(
            parse_offset_identifier_nanoseconds("+01:00"),
            Some(3_600_000_000_000)
        );
        assert_eq!(
            parse_offset_identifier_nanoseconds("-0130"),
            Some(-5_400_000_000_000)
        );
        assert_eq!(parse_offset_identifier_nanoseconds("Z"), None);
        assert_eq!(parse_offset_identifier_nanoseconds("+25:00"), None);
        assert_eq!(parse_offset_identifier_nanoseconds("+00:00junk"), None);
        // An identifier stays minute-precision: sub-minute belongs to a
        // string's own offset, never to its zone annotation.
        assert_eq!(parse_offset_identifier_nanoseconds("-07:00:01"), None);
    }

    #[test]
    fn parses_duration_strings_with_a_fraction_on_the_smallest_unit_present() {
        let fields = |source: &str| {
            parse_duration_record(source).map(|record| {
                [
                    record.years,
                    record.months,
                    record.weeks,
                    record.days,
                    record.hours,
                    record.minutes,
                    record.seconds,
                    record.milliseconds,
                    record.microseconds,
                    record.nanoseconds,
                ]
            })
        };
        // Temporal/Duration/from/argument-string.js.
        assert_eq!(fields("P1D"), Some([0, 0, 0, 1, 0, 0, 0, 0, 0, 0]));
        assert_eq!(
            fields("p1y1m1dt1h1m1s"),
            Some([1, 1, 0, 1, 1, 1, 1, 0, 0, 0])
        );
        assert_eq!(
            fields("P1Y1M1W1DT1H1M1.123456789S"),
            Some([1, 1, 1, 1, 1, 1, 1, 123, 456, 789])
        );
        assert_eq!(
            fields("P1Y1M1W1DT1H1M1,12S"),
            Some([1, 1, 1, 1, 1, 1, 1, 120, 0, 0])
        );
        assert_eq!(fields("P1DT0.5M"), Some([0, 0, 0, 1, 0, 0, 30, 0, 0, 0]));
        assert_eq!(fields("P1DT0,5H"), Some([0, 0, 0, 1, 0, 30, 0, 0, 0, 0]));
        assert_eq!(fields("+P1D"), Some([0, 0, 0, 1, 0, 0, 0, 0, 0, 0]));
        assert_eq!(
            fields("-P1Y1M1W1DT1H1M1.123456789S"),
            Some([-1, -1, -1, -1, -1, -1, -1, -123, -456, -789])
        );
        assert_eq!(fields("PT100M"), Some([0, 0, 0, 0, 0, 100, 0, 0, 0, 0]));
        // Temporal/Duration/from/string-with-skipped-units.js.
        assert_eq!(fields("P3Y4W"), Some([3, 0, 4, 0, 0, 0, 0, 0, 0, 0]));
        assert_eq!(
            fields("PT3H4.123456789S"),
            Some([0, 0, 0, 0, 3, 0, 4, 123, 456, 789])
        );
        // Temporal/Duration/from/argument-string-fractional-precision.js:
        // fractions are exact, not floating point.
        assert_eq!(
            fields("PT0.999999999H"),
            Some([0, 0, 0, 0, 0, 59, 59, 999, 996, 400])
        );
        assert_eq!(
            fields("PT0.000000011H"),
            Some([0, 0, 0, 0, 0, 0, 0, 0, 39, 600])
        );
        assert_eq!(
            fields("PT0.999999999M"),
            Some([0, 0, 0, 0, 0, 0, 59, 999, 999, 940])
        );
        assert_eq!(
            fields("PT0.000000011M"),
            Some([0, 0, 0, 0, 0, 0, 0, 0, 0, 660])
        );
        // Temporal/Duration/from/argument-string-negative-fractional-units.js.
        assert_eq!(
            fields("-PT24.567890123H"),
            Some([0, 0, 0, 0, -24, -34, -4, -404, -442, -800])
        );
        assert_eq!(
            fields("-PT1440.567890123M"),
            Some([0, 0, 0, 0, 0, -1440, -34, -73, -407, -380])
        );
    }

    #[test]
    fn rejects_duration_strings_that_break_the_grammar() {
        // Temporal/Duration/from/argument-string-invalid.js and
        // argument-string-fractional-with-zero-subparts.js.
        for source in [
            "P1Y1M1W1DT1H1M1.123456789123S",
            "P0.5Y",
            "P1Y0,5M",
            "P1Y1M0.5W",
            "P1Y1M1W0,5D",
            "P1Y1M1W1DT0.5H5S",
            "P1Y1M1W1DT1.5H0,5M",
            "P1Y1M1W1DT1H0.5M0.5S",
            "P",
            "PT",
            "-P",
            "-PT",
            "+P",
            "+PT",
            "P1Y1M1W1DT1H1M1.01Sjunk",
            "P-1Y1M",
            "P1Y-1M",
            "P2H",
            "P2.5M",
            "P2S",
            "PT2.H3M",
            "PT2H3.2M3S",
            "PT.1H",
            "PT,1S",
            "PT0.1H0M",
            "PT0.1H0.0M",
            "PT0.1M0S",
            "",
            "not-a-duration",
            // Ordering and repetition are both fixed by the grammar.
            "P1D1Y",
            "P1Y1Y",
            "PT1S1H",
            "P1DT1HT1M",
            // Temporal/Duration/from/argument-string-is-infinity.js: a
            // component too large to represent is out of range.
            "P999999999999999999999999999999999999999999Y",
        ] {
            assert_eq!(parse_duration_record(source), None, "{source}");
        }
    }

    #[test]
    fn keeps_representable_range_checks_out_of_the_grammar() {
        // Temporal/PlainYearMonth/from/argument-string-limits.js: the
        // year-month range is a month wider than the date range at each end.
        assert!(is_year_month_within_limits(-271_821, 4));
        assert!(!is_year_month_within_limits(-271_821, 3));
        assert!(is_year_month_within_limits(275_760, 9));
        assert!(!is_year_month_within_limits(275_760, 10));
        assert!(!is_year_month_within_limits(999_999, 1));
        assert!(is_year_month_within_limits(1976, 11));
        // The grammar itself accepts any 4- or 6-digit year, leaving the
        // limit to the type that cares: PlainMonthDay accepts
        // "-999999-10-01" that PlainDate must reject.
        assert_eq!(date("+999999-01-01"), Some((999_999, 1, 1)));
    }
}
