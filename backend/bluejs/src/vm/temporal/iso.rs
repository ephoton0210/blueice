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

use super::epoch::{CivilDate, CivilTime};

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

/// A two-digit, all-ASCII-digit field. Every ISO date and time component
/// Temporal accepts is exactly two digits wide (the extended year being the
/// sole exception), so rejecting any other width here is what makes
/// `00:0000` and `0000:00` the syntax errors the grammar says they are.
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

/// Parses an `HH:MM:SS.fraction` time-of-day, ignoring any UTC offset,
/// designator or annotation that follows it.
pub(crate) fn parse_time(source: &str) -> Option<(u8, u8, u8, u16, u16, u16)> {
    parse_iso_time_prefix(source).map(|(time, _)| time)
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

/// [`parse_annotation_suffix`], keeping only the resolved calendar.
pub(crate) fn parse_annotations(cursor: &str) -> Result<Option<String>, ()> {
    parse_annotation_suffix(cursor).map(|annotations| annotations.calendar)
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
/// carried by a full ISO date-time string, into a fixed UTC offset in
/// nanoseconds.
///
/// `Ok(None)` means the identifier is syntactically valid but names an IANA
/// zone whose transition rules this engine cannot yet resolve (everything
/// except `UTC`); `Err(())` means the input is not a time zone at all.
/// Resolving named zones is Stage 1 Track E's `time_zone.rs`.
pub(crate) fn resolve_fixed_time_zone_offset(source: &str) -> Result<Option<i128>, ()> {
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
                return Ok(Some(offset.nanoseconds));
            }
        }
    };
    if let Some(offset) = parse_minute_precision_offset(&identifier) {
        return Ok(Some(offset));
    }
    if identifier.eq_ignore_ascii_case("UTC") {
        return Ok(Some(0));
    }
    Ok(None)
}

/// Parses the `TemporalTimeString` grammar — the string form
/// `Temporal.PlainTime.from`/`ToTemporalTime` accept.
///
/// This is deliberately not `parse_time` on its own: a PlainTime string may
/// carry a full date prefix (`1976-11-18T12:34`, including a bare space as
/// the separator), a leading `T`/`t` time designator, a UTC offset and
/// bracket annotations — while a *date-only* string must be rejected rather
/// than implicitly meaning midnight, and a UTC designator (`Z`) is not valid
/// on a PlainTime at all. Every one of those rules is pinned by Test262
/// fixtures under `built-ins/Temporal/PlainTime/` (`argument-string-*`).
///
/// Known gap: the basic (separator-less) format — `T1214` for `12:14` — is
/// not accepted, consistent with the rest of this parser and with Phase 26's
/// Stage 0 note that Temporal restricts itself to the extended format. The
/// separator-less strings Test262 exercises here are the *ambiguous* ones a
/// PlainTime must reject anyway (`1214`, `202112`), which this does; only
/// their `T`-prefixed unambiguous counterparts are missed.
/// Whether `body` would *also* parse as a PlainMonthDay or PlainYearMonth,
/// which the grammar calls ambiguous: a bare (un-`T`-prefixed) PlainTime
/// string may not shadow one, so it has to be rejected rather than silently
/// read as a time.
///
/// The four shapes, in both the extended and the basic form (`MM-DD`/`MMDD`
/// against `HH-UU`/`HHMM`, and `YYYY-MM`/`YYYYMM` against `HHMM-UU`/
/// `HHMMSS`), are exactly the ones Test262's
/// `TemporalHelpers.ISO.plainTimeStringsAmbiguous()` lists; its
/// `plainTimeStringsUnambiguous()` counterpart is what pins the *validity*
/// checks here (`0230` is February 30th, so it is not a real date and stays a
/// valid time; `0229` is, so it is ambiguous).
fn is_ambiguous_with_a_date(body: &str) -> bool {
    let month_day = |month: u8, day: u8| {
        // 1972 is Test262's own reference year for a PlainMonthDay, which
        // matters for exactly one case: February 29th exists there.
        days_in_month(1972, month).is_some_and(|last| (1..=last).contains(&day))
    };
    let is_month = |month: u8| (1..=12).contains(&month);
    let digits = |text: &str| text.bytes().all(|byte| byte.is_ascii_digit());
    match body.split_once('-') {
        Some((left, right)) => match (left.len(), two_digit_field(right)) {
            (2, Some(right)) => two_digit_field(left).is_some_and(|left| month_day(left, right)),
            (4, Some(right)) => digits(left) && is_month(right),
            _ => false,
        },
        None if digits(body) => match body.len() {
            4 => matches!(
                (two_digit_field(&body[..2]), two_digit_field(&body[2..])),
                (Some(month), Some(day)) if month_day(month, day)
            ),
            6 => two_digit_field(&body[4..]).is_some_and(is_month),
            _ => false,
        },
        None => false,
    }
}

pub(crate) fn parse_plain_time(source: &str) -> Option<(u8, u8, u8, u16, u16, u16)> {
    let (body, annotations) = match source.find('[') {
        Some(index) => (&source[..index], &source[index..]),
        None => (source, ""),
    };
    // A PlainTime ignores any calendar the annotation names (even an unknown
    // one), but its *syntax* is still validated.
    parse_annotations(annotations).ok()?;
    let time = if let Some(rest) = body.strip_prefix(['T', 't']) {
        rest
    } else {
        match body.find(['T', 't', ' ']) {
            Some(index) if parse_date(&body[..index]).is_some() => &body[index + 1..],
            // No date prefix: the whole body is the time of day. A date-only
            // body falls here too and fails `parse_time` below, which is
            // exactly the "no implicit midnight" rule.
            _ => {
                // ...except where a time-shaped body would *also* be a valid
                // PlainMonthDay, which the grammar calls ambiguous and
                // requires a `T` designator to resolve: `12-14` could be
                // December 14th or 12:00 at offset -14:00, so a PlainTime
                // must reject it rather than silently pick the latter
                // (Test262's
                // `argument-string-time-designator-required-for-disambiguation.js`).
                if is_ambiguous_with_a_date(body) {
                    return None;
                }
                body
            }
        }
    };
    if time.contains(['Z', 'z']) {
        return None;
    }
    // The offset itself is ignored by a PlainTime, but a malformed one (or
    // trailing junk after it) is still a syntax error.
    if time.contains(['+', '-']) && parse_offset_seconds(time).is_none() {
        return None;
    }
    parse_time(time)
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

    #[test]
    fn parses_calendar_dates_within_the_supported_range() {
        assert_eq!(parse_date("2000-05-02"), Some((2000, 5, 2)));
        assert_eq!(parse_date("+002020-06-01"), Some((2020, 6, 1)));
        assert_eq!(parse_date("2000-05-02T15:23"), Some((2000, 5, 2)));
        assert_eq!(parse_date("2000-02-30"), None);
        assert_eq!(parse_date("2000-13-01"), None);
        assert_eq!(parse_date("not-a-date"), None);
    }

    #[test]
    fn parses_time_of_day_with_fractional_seconds() {
        assert_eq!(parse_time("15:23"), Some((15, 23, 0, 0, 0, 0)));
        assert_eq!(
            parse_time("15:23:07.008009010"),
            Some((15, 23, 7, 8, 9, 10))
        );
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("15:60"), None);
    }

    #[test]
    fn parses_basic_format_and_comma_separated_times() {
        // Test262's `PlainTime/from/argument-string.js`: the separator-less
        // basic format and `,` as the decimal separator are both accepted,
        // and a lone two-digit hour is a complete time.
        assert_eq!(parse_time("152330"), Some((15, 23, 30, 0, 0, 0)));
        assert_eq!(parse_time("152330.1"), Some((15, 23, 30, 100, 0, 0)));
        assert_eq!(parse_time("0030"), Some((0, 30, 0, 0, 0, 0)));
        assert_eq!(parse_time("15"), Some((15, 0, 0, 0, 0, 0)));
        assert_eq!(parse_time("15:23:30,12"), Some((15, 23, 30, 120, 0, 0)));
        assert_eq!(parse_time("152330-0800"), Some((15, 23, 30, 0, 0, 0)));
    }

    #[test]
    fn rejects_inconsistent_or_overlong_time_fields() {
        // Test262's `PlainTime/from/argument-string-invalid.js`: the
        // separators either all appear or none do, every field is exactly two
        // digits, and no fraction may exceed nine digits or sit on a field
        // other than the seconds.
        for source in [
            "",
            "001",
            "01:001",
            "0000:00",
            "00:0000",
            "00:00:00:00",
            "00:",
            "0:0",
            "1",
            "00:00:00.",
            "00:00:00.1234567891",
            "05:07.123",
            "12.5",
            "00:61",
            "24:00:00",
            "00:00:61",
        ] {
            assert_eq!(parse_time(source), None, "{source}");
        }
    }

    #[test]
    fn parses_basic_format_dates_and_rejects_a_negative_zero_year() {
        // Test262's `PlainTime/from/argument-string.js` (basic-format date
        // prefixes) and `.../year-zero.js`.
        assert_eq!(parse_date("19761118"), Some((1976, 11, 18)));
        assert_eq!(parse_date("+0019761118"), Some((1976, 11, 18)));
        assert_eq!(parse_date("+0019761118T15:23"), Some((1976, 11, 18)));
        assert_eq!(parse_date("-000000-12-07"), None);
        assert_eq!(parse_date("-000000-12-07T03:24:30"), None);
        assert_eq!(parse_date("+000000-12-07"), Some((0, 12, 7)));
        // A six-digit year needs its sign, and a four-digit one must not
        // carry one.
        assert_eq!(parse_date("002020-06-01"), None);
        assert_eq!(parse_date("+2020-06-01"), None);
    }

    #[test]
    fn parses_offsets_with_fractional_seconds_and_rejects_bad_ones() {
        // Test262's `PlainTime/prototype/until/argument-string-date-with-utc-offset.js`
        // valid list, and `from/argument-string-invalid.js`'s offsets.
        assert_eq!(parse_offset_seconds("+00:00:00,0"), Some(0));
        assert_eq!(parse_offset_seconds("+000000.000000000"), Some(0));
        assert_eq!(parse_offset_seconds("-023000,0"), Some(-9_000));
        for source in [
            "+24:00",
            "-24:00",
            "+00:0000",
            "+0000:00",
            "+00:00:00.1234567891",
            "+00:00junk",
        ] {
            assert_eq!(parse_offset_seconds(source), None, "{source}");
        }
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
    fn rejects_non_plain_time_strings() {
        for source in [
            // A date-only string never implicitly means midnight.
            "2019-10-01",
            "2000-05-02[UTC]",
            // A UTC designator is not valid on a PlainTime.
            "09:00:00Z",
            "09:00:00Z[UTC]",
            "2019-10-01T09:00:00Z",
            "2022-09-15+00:00",
            // Ambiguous with PlainYearMonth/PlainMonthDay without a `T`.
            "2021-12",
            "12-14",
            "202112[UTC]",
            // Fractional minutes/hours, over-long fractions, trailing junk.
            "05:07.123",
            "12.5",
            "00:00:00.1234567891",
            "15:23:30.100junk",
            // Annotation syntax errors.
            "00:00[u-ca=iso8601][!u-ca=iso8601]",
            "00:00[UTC][UTC]",
            "00:00[!foo=bar]",
            "",
            "T",
        ] {
            assert_eq!(parse_plain_time(source), None, "{source}");
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
    fn resolves_only_utc_and_fixed_offset_time_zones() {
        for (source, expected) in [
            ("UTC", Ok(Some(0))),
            ("utc", Ok(Some(0))),
            ("+01:00", Ok(Some(3_600_000_000_000))),
            ("-01:30", Ok(Some(-5_400_000_000_000))),
            ("2021-08-19T17:30Z", Ok(Some(0))),
            ("2021-08-19T17:30-07:00", Ok(Some(-25_200_000_000_000))),
            ("2021-08-19T17:30-07:00[UTC]", Ok(Some(0))),
            (
                "2021-08-19T17:30:45.123456789-12:12[+01:46]",
                Ok(Some(6_360_000_000_000)),
            ),
            ("2016-12-31T23:59:60+00:00[UTC]", Ok(Some(0))),
            // Syntactically a zone, but its transition rules are Track E's.
            ("Europe/Vienna", Ok(None)),
            ("Mars/Olympus_Mons", Ok(None)),
            // Not a time zone at all.
            ("", Err(())),
            ("2021-08-19T17:30", Err(())),
            ("2021-08-19T17:30-07:00:01", Err(())),
            ("2021-08-19T17:30-07:00:00", Err(())),
            ("2021-08-19T17:30:45.123456789+23:59[+23:59:60]", Err(())),
        ] {
            assert_eq!(
                resolve_fixed_time_zone_offset(source),
                expected,
                "{source:?}"
            );
        }
    }
}
