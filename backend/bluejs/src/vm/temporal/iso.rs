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

/// `UTCOffset`: a sign, then the same shape as a time. With
/// `sub_minute` cleared this is `UTCOffsetMinutePrecision`, the only form a
/// time-zone annotation may use
/// (`Temporal/Instant/from/instant-string-sub-minute-offset.js`).
/// Returns nanoseconds east of UTC.
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

/// One `DurationDate`/`DurationTime` term: a digit run, an optional
/// fraction, and the unit designator that closes it.
struct DurationTerm {
    number: i128,
    fraction: Option<u32>,
    designator: u8,
    in_time: bool,
}

/// Parses the ISO duration strings accepted by `Intl.DurationFormat` and
/// `Temporal.Duration`. The host service receives a typed, validated
/// ECMA-402 record, so neither this parser nor a Temporal object can
/// trigger observable duration-field accessors while formatting.
///
/// Temporal's duration grammar is narrower than general ISO 8601 in that a
/// fraction may only appear on the **last** component present, but it is
/// not restricted to seconds: `P1DT0.5M` is half a minute and `P1DT0,5H`
/// half an hour (`Temporal/Duration/from/argument-string.js`), while
/// `PT0.1H0M` is a syntax error
/// (`Duration/from/argument-string-fractional-with-zero-subparts.js`).
/// Fractions convert exactly, never through floating point
/// (`argument-string-fractional-precision.js`).
pub(crate) fn parse_duration_record(source: &str) -> Option<blueice_ecma402::DurationRecord> {
    let mut cursor = Cursor::new(source);
    let sign: i128 = match cursor.eat_any(b"+-") {
        Some(b'-') => -1,
        _ => 1,
    };
    cursor.eat_any(b"Pp")?;
    let mut terms: Vec<DurationTerm> = Vec::new();
    let mut in_time = false;
    while !cursor.done() {
        if cursor.eat_any(b"Tt").is_some() {
            if in_time {
                return None;
            }
            in_time = true;
            continue;
        }
        let start = cursor.index;
        while cursor.peek_digit() {
            cursor.index += 1;
        }
        if cursor.index == start {
            return None;
        }
        let number = source[start..cursor.index].parse::<i128>().ok()?;
        let fraction = if cursor.eat_any(b".,").is_some() {
            Some(cursor.fraction_nanoseconds()?)
        } else {
            None
        };
        let designator = cursor.peek()?.to_ascii_uppercase();
        cursor.index += 1;
        terms.push(DurationTerm {
            number,
            fraction,
            designator,
            in_time,
        });
    }
    if terms.is_empty() || (in_time && !terms.iter().any(|term| term.in_time)) {
        return None;
    }
    let mut values = [0_i128; 10];
    let mut date_order = 0;
    let mut time_order = 0;
    for (position, term) in terms.iter().enumerate() {
        let last = position + 1 == terms.len();
        let index = if term.in_time {
            let order = 1 + b"HMS".iter().position(|unit| *unit == term.designator)?;
            if order <= time_order {
                return None;
            }
            time_order = order;
            3 + order
        } else {
            let order = 1 + b"YMWD".iter().position(|unit| *unit == term.designator)?;
            if order <= date_order {
                return None;
            }
            date_order = order;
            order - 1
        };
        values[index] = term.number;
        if let Some(fraction) = term.fraction {
            // A fraction is only legal on the final component, and a date
            // component never takes one at all.
            if !last || !term.in_time {
                return None;
            }
            let mut nanoseconds = i128::from(fraction)
                * match term.designator {
                    b'H' => 3_600,
                    b'M' => 60,
                    _ => 1,
                };
            if term.designator == b'H' {
                values[5] = nanoseconds / 60_000_000_000;
                nanoseconds %= 60_000_000_000;
            }
            if term.designator != b'S' {
                values[6] = nanoseconds / 1_000_000_000;
                nanoseconds %= 1_000_000_000;
            }
            values[7] = nanoseconds / 1_000_000;
            values[8] = (nanoseconds / 1_000) % 1_000;
            values[9] = nanoseconds % 1_000;
        }
    }
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
    fn resolves_the_first_calendar_annotation_and_ignores_later_ones() {
        let calendar = |source: &str| parse_date_time(source).map(|parsed| parsed.calendar);
        assert_eq!(
            calendar("2000-05-02[u-ca=hebrew]"),
            Some(Some("hebrew".to_string()))
        );
        assert_eq!(
            calendar("2000-05-02[u-ca=hebrew][u-ca=discord]"),
            Some(Some("hebrew".to_string()))
        );
        assert_eq!(
            calendar("2000-05-02[UTC][u-ca=hebrew]"),
            Some(Some("hebrew".to_string()))
        );
        assert_eq!(calendar("2000-05-02[foo=bar]"), Some(None));
        assert_eq!(calendar("2000-05-02"), Some(None));
        assert_eq!(
            parse_date_time("2000-05-02T15:23[!Europe/Vienna]")
                .unwrap()
                .time_zone,
            Some("Europe/Vienna".to_string())
        );
        assert_eq!(
            parse_date_time("2000-05-02T15:23[+00:00]")
                .unwrap()
                .time_zone,
            Some("+00:00".to_string())
        );
        assert_eq!(
            parse_date_time("1976-11-18T15:23:30.123456789Z[NotATimeZone]")
                .unwrap()
                .time_zone,
            Some("NotATimeZone".to_string())
        );
    }

    #[test]
    fn rejects_invalid_or_critical_unknown_annotations() {
        for source in [
            // Critical unknown keys, and always-invalid uppercase keys.
            "2000-05-02[!foo=bar]",
            "2000-05-02[FOO=bar]",
            "2000-05-02[u-CA=iso8601]",
            "2000-05-02[U-CA=iso8601]",
            "2000-05-02[u-ca=]",
            "2000-05-02[=iso8601]",
            "2000-05-02[u-ca=iso8601",
            // A repeated calendar annotation is only tolerated when none is
            // critical: PlainDateTime/from/argument-string-multiple-calendar.js.
            "1970-01-01[u-ca=iso8601][!u-ca=iso8601]",
            "1970-01-01[!u-ca=iso8601][u-ca=iso8601]",
            "1970-01-01[UTC][u-ca=iso8601][!u-ca=iso8601]",
            "1970-01-01[u-ca=iso8601][foo=bar][!u-ca=iso8601]",
            // Only one time-zone annotation, and only in first position:
            // Instant/from/argument-string-multiple-time-zone.js.
            "1970-01-01T00:00Z[UTC][UTC]",
            "1970-01-01T00:00Z[!UTC][UTC]",
            "1970-01-01T00:00Z[UTC][u-ca=iso8601][UTC]",
            // A time-zone annotation may not carry a sub-minute offset:
            // Instant/from/instant-string-sub-minute-offset.js.
            "2021-08-19T17:30-07:00:01[-07:00:01]",
            "2021-08-19T17:30-07:00:00[-070000]",
            // Trailing junk after each syntactic position.
            "2020-01-01T00:00:00+00:00junk",
            "2020-01-01T00:00:00+00:00[UTC]junk",
            "2020-01-01T00:00:00+00:00[UTC][u-ca=iso8601]junk",
            "2020-01-01T00:00Zjunk",
        ] {
            assert_eq!(parse_date_time(source), None, "{source}");
        }
        // An unknown annotation is ignored when it is not critical, and a
        // long mixed-case value is legal: Instant/from/argument-string-unknown-annotation.js.
        assert!(
            parse_date_time("1970-01-01T00:00Z[foo=bar][_foo-bar0=Ignore-This-999999999999]")
                .is_some()
        );
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
