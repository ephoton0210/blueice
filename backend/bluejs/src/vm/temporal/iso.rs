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

/// Parses the `YYYY-MM-DD` date portion at the start of an ISO
/// date/date-time/instant string, in either the extended (separated) or the
/// basic (separator-less) form, with a four-digit unsigned year or a
/// sign-plus-six-digit extended year.
pub(crate) fn parse_date(source: &str) -> Option<(i32, u8, u8)> {
    let end = source
        .find(['T', 't', '[', 'Z', 'z'])
        .unwrap_or(source.len());
    let date = &source[..end];
    let (sign, rest) = match date.as_bytes().first() {
        Some(b'+') => (1_i32, &date[1..]),
        Some(b'-') => (-1_i32, &date[1..]),
        _ => (1, date),
    };
    // A sign is exactly what distinguishes the six-digit extended year from
    // the plain four-digit one; neither form may borrow the other's width.
    let signed = date.len() != rest.len();
    let year_width = if signed { 6 } else { 4 };
    let (year, month, day) = if rest.contains('-') {
        let mut fields = rest.split('-');
        let year = fields.next()?;
        let month = fields.next()?;
        let day = fields.next()?;
        if fields.next().is_some() {
            return None;
        }
        (year, month, day)
    } else {
        (
            rest.get(..year_width)?,
            rest.get(year_width..year_width + 2)?,
            rest.get(year_width + 2..)?,
        )
    };
    if year.len() != year_width || !year.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let magnitude: i32 = year.parse().ok()?;
    // `-000000` is a negative zero year, which the grammar rejects outright.
    if sign < 0 && magnitude == 0 {
        return None;
    }
    let year = sign * magnitude;
    let month = two_digit_field(month)?;
    let day = two_digit_field(day)?;
    ((-271_821..=275_760).contains(&year)
        && day >= 1
        && days_in_month(year, month).is_some_and(|last| day <= last))
    .then_some((year, month, day))
}

/// Parses an `HH:MM:SS.fraction` time-of-day (ignoring any UTC offset,
/// designator or annotation that follows it), returning
/// `(hour, minute, second, millisecond, microsecond, nanosecond)`.
pub(crate) fn parse_time(source: &str) -> Option<(u8, u8, u8, u16, u16, u16)> {
    let body = source
        .split(['Z', 'z', '+', '-', '['])
        .next()
        .unwrap_or(source);
    let (hour, minute, second, nanos) = parse_time_spec(body)?;
    if second > 60 {
        return None;
    }
    // `ParseISODateTime` accepts a `:60` leap second in the grammar and
    // immediately constrains it to `:59` (there is no leap second in
    // Temporal's time record) — Test262's
    // `PlainTime/from/argument-string-leap-second.js`.
    let second = second.min(59);
    Some((
        hour,
        minute,
        second,
        (nanos / 1_000_000) as u16,
        ((nanos / 1_000) % 1_000) as u16,
        (nanos % 1_000) as u16,
    ))
}

/// Scans the zero-or-more bracket annotations that may follow an ISO
/// date/time/offset prefix, returning the first `u-ca=` value if present.
///
/// Grammar notes (from Temporal's annotation syntax): an optional leading
/// time-zone annotation (no `=` in its body) is skipped without further
/// validation here — resolving it is a separate, later concern (Stage 1
/// Track E's `time_zone.rs`, and matching `Intl.DateTimeFormat`'s own
/// time-zone-annotation handling elsewhere in this codebase). Every
/// subsequent annotation is `[!]key=value`; a key containing any
/// non-lowercase character is always a syntax error, regardless of the
/// critical (`!`) flag. A second or later `u-ca` annotation is always
/// ignored, never validated. Any other unrecognized key is ignored unless
/// marked critical, in which case this returns `Err`.
pub(crate) fn parse_annotations(mut cursor: &str) -> Result<Option<String>, ()> {
    if let Some(rest) = cursor.strip_prefix('[') {
        let end = rest.find(']').ok_or(())?;
        let body = rest[..end].strip_prefix('!').unwrap_or(&rest[..end]);
        if !body.contains('=') {
            cursor = &rest[end + 1..];
        }
    }
    let mut calendar = None;
    let mut calendar_count = 0_usize;
    let mut any_critical = false;
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
            calendar_count += 1;
            any_critical |= critical;
            calendar.get_or_insert_with(|| value.to_string());
        } else if critical {
            return Err(());
        }
    }
    // A repeated calendar annotation is ordinarily ignored, but repeating it
    // with the critical flag anywhere in the set is a syntax error.
    if calendar_count > 1 && any_critical {
        return Err(());
    }
    Ok(calendar)
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
pub(crate) fn parse_duration_record(source: &str) -> Option<blueice_ecma402::DurationRecord> {
    let (sign, source) = match source.as_bytes().first() {
        Some(b'+') => (1_i128, &source[1..]),
        Some(b'-') => (-1_i128, &source[1..]),
        _ => (1_i128, source),
    };
    let mut characters = source.chars().peekable();
    (characters.next() == Some('P')).then_some(())?;
    let mut values = [0_i128; 10];
    let mut in_time = false;
    let mut saw_component = false;
    while characters.peek().is_some() {
        if characters.peek() == Some(&'T') {
            if in_time {
                return None;
            }
            characters.next();
            in_time = true;
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
        if characters.peek() == Some(&'.') {
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
        let designator = characters.next()?;
        let index = match (in_time, designator) {
            (false, 'Y') => 0,
            (false, 'M') => 1,
            (false, 'W') => 2,
            (false, 'D') => 3,
            (true, 'H') => 4,
            (true, 'M') => 5,
            (true, 'S') => 6,
            _ => return None,
        };
        if let Some(fraction) = fraction {
            if designator != 'S' {
                return None;
            }
            let fraction = format!("{fraction:0<9}").parse::<i128>().ok()?;
            values[7] = fraction / 1_000_000;
            values[8] = (fraction / 1_000) % 1_000;
            values[9] = fraction % 1_000;
        }
        values[index] = number.parse::<i128>().ok()?;
        saw_component = true;
    }
    saw_component.then_some(())?;
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

/// Parses a UTC-offset or `Z`/`z` suffix (`temporal_offset_seconds`'s own
/// former name) into signed seconds east of UTC.
pub(crate) fn parse_offset_seconds(source: &str) -> Option<i32> {
    let index = source.char_indices().find_map(|(index, character)| {
        matches!(character, 'Z' | 'z' | '+' | '-' | '[').then_some(index)
    })?;
    let suffix = &source[index..];
    if matches!(suffix.as_bytes().first(), Some(b'Z' | b'z')) {
        return (suffix.len() == 1 || suffix.starts_with("Z[") || suffix.starts_with("z["))
            .then_some(0);
    }
    let sign = match suffix.as_bytes().first() {
        Some(b'+') => 1,
        Some(b'-') => -1,
        _ => return None,
    };
    let fields = suffix[1..]
        .split_once('[')
        .map_or(&suffix[1..], |(fields, _)| fields);
    // An offset shares the time-of-day grammar, including a sub-minute
    // fraction (`+00:00:00.000000000`). The fractional part is dropped: no
    // real zone offset has sub-second precision, and Temporal's own
    // `PlainTime` path ignores the offset entirely — it is parsed only so a
    // malformed one is still a syntax error.
    let (hour, minute, second, _) = parse_time_spec(fields)?;
    (second <= 59)
        .then_some(sign * (i32::from(hour) * 3_600 + i32::from(minute) * 60 + i32::from(second)))
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
    fn parses_duration_strings_with_the_seconds_only_fraction_rule() {
        assert!(parse_duration_record("P1Y2M3W4DT5H6M7.008009010S").is_some());
        assert!(parse_duration_record("P1DT2H30.5M").is_none());
        assert!(parse_duration_record("not-a-duration").is_none());
    }

    #[test]
    fn parses_offsets_including_zulu_and_bracket_termination() {
        assert_eq!(parse_offset_seconds("Z"), Some(0));
        assert_eq!(parse_offset_seconds("+01:00"), Some(3_600));
        assert_eq!(parse_offset_seconds("-0130"), Some(-5_400));
        assert_eq!(parse_offset_seconds("+25:00"), None);
    }
}
