// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Byte-cursor scanning primitives for Temporal's date and time grammar:
//! the shared [`Cursor`], `scan_*` field scanners, `parse_time_spec`, and the
//! date/time *prefix* parsers other modules build on.

use super::datetime::Time;
use super::days_in_month;
use crate::vm::temporal::epoch::{CivilDate, CivilTime};

/// A byte cursor over an ISO string. Every character in Temporal's grammar
/// is ASCII, so byte-wise scanning both matches the grammar exactly and
/// rejects look-alikes such as the Unicode minus sign U+2212 for free.
pub(super) struct Cursor<'a> {
    source: &'a str,
    index: usize,
}

impl<'a> Cursor<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        Self { source, index: 0 }
    }

    pub(super) fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.index).copied()
    }

    pub(super) fn peek_digit(&self) -> bool {
        self.peek().is_some_and(|byte| byte.is_ascii_digit())
    }

    pub(super) fn done(&self) -> bool {
        self.index >= self.source.len()
    }

    pub(super) fn eat(&mut self, byte: u8) -> bool {
        let matched = self.peek() == Some(byte);
        if matched {
            self.index += 1;
        }
        matched
    }

    pub(super) fn eat_any(&mut self, bytes: &[u8]) -> Option<u8> {
        let byte = self.peek()?;
        bytes.contains(&byte).then(|| {
            self.index += 1;
            byte
        })
    }

    /// Consumes exactly `count` ASCII digits, or nothing.
    pub(super) fn digits(&mut self, count: usize) -> Option<u32> {
        let text = self.source.get(self.index..self.index + count)?;
        if !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        self.index += count;
        text.parse().ok()
    }

    /// Consumes a `TemporalDecimalFraction`'s digits (1 to 9 of them, a
    /// 10th being a syntax error) and returns them scaled to nanoseconds.
    pub(super) fn fraction_nanoseconds(&mut self) -> Option<u32> {
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
    pub(super) fn peek_bracket(&self) -> Option<&'a str> {
        let rest = self.source.get(self.index..)?.strip_prefix('[')?;
        rest.find(']').map(|end| &rest[..end])
    }

    /// Consumes the bracketed annotation at the cursor, returning its body.
    pub(super) fn take_bracket(&mut self) -> Option<&'a str> {
        let body = self.peek_bracket()?;
        self.index += body.len() + 2;
        Some(body)
    }
}

/// `DateYear`: 4 digits, or a sign and exactly 6. A signed zero year is
/// rejected (`Temporal/PlainDate/from/year-zero.js`).
pub(super) fn scan_year(cursor: &mut Cursor) -> Option<i32> {
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
pub(super) fn scan_date(cursor: &mut Cursor) -> Option<(i32, u8, u8)> {
    let year = scan_year(cursor)?;
    let extended = cursor.eat(b'-');
    let month = cursor.digits(2)? as u8;
    if cursor.eat(b'-') != extended {
        return None;
    }
    let day = cursor.digits(2)? as u8;
    is_valid_date(year, month, day).then_some((year, month, day))
}

pub(super) fn is_valid_date(year: i32, month: u8, day: u8) -> bool {
    day >= 1 && days_in_month(year, month).is_some_and(|last| day <= last)
}

/// `TimeSpec`: `Hour [[':'] Minute [[':'] Second [Fraction]]]`, with the
/// separator choice consistent throughout. A `60` seconds field is a leap
/// second and parses as `59`, per `ParseISODateTime`.
pub(super) fn scan_time(cursor: &mut Cursor) -> Option<Time> {
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

pub(super) fn two_digit_field(text: &str) -> Option<u8> {
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
pub(super) fn parse_time_spec(source: &str) -> Option<(u8, u8, u8, u32)> {
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

/// Splits exactly `count` leading ASCII digits off `source`.
pub(super) fn split_digits(source: &str, count: usize) -> Option<(&str, &str)> {
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
