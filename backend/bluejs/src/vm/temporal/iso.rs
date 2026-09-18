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

/// Parses the `YYYY-MM-DD` (optionally sign-sextet-year) date portion at
/// the start of an ISO date/date-time/instant string.
pub(crate) fn parse_date(source: &str) -> Option<(i32, u8, u8)> {
    let end = source
        .find(['T', 't', '[', 'Z', 'z'])
        .unwrap_or(source.len());
    let date = &source[..end];
    let end = date.rfind('-')?;
    let before_day = &date[..end];
    let middle = before_day.rfind('-')?;
    let year = date[..middle].parse().ok()?;
    let month = date[middle + 1..end].parse().ok()?;
    let day = date[end + 1..].parse().ok()?;
    ((-271_821..=275_760).contains(&year)
        && days_in_month(year, month).is_some_and(|last| day <= last))
    .then_some((year, month, day))
}

/// Parses an `HH:MM:SS.fraction` time-of-day, returning
/// `(hour, minute, second, millisecond, microsecond, nanosecond)`.
pub(crate) fn parse_time(source: &str) -> Option<(u8, u8, u8, u16, u16, u16)> {
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
            calendar.get_or_insert_with(|| value.to_string());
        } else if critical {
            return Err(());
        }
    }
    Ok(calendar)
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
    let fields: Vec<_> = if fields.contains(':') {
        fields.split(':').collect()
    } else {
        match fields.len() {
            2 => vec![&fields[..2]],
            4 => vec![&fields[..2], &fields[2..4]],
            6 => vec![&fields[..2], &fields[2..4], &fields[4..6]],
            _ => return None,
        }
    };
    let [hour, minute, second] = match fields.as_slice() {
        [hour] => [*hour, "0", "0"],
        [hour, minute] => [*hour, *minute, "0"],
        [hour, minute, second] => [*hour, *minute, *second],
        _ => return None,
    };
    let hour: i32 = hour.parse().ok()?;
    let minute: i32 = minute.parse().ok()?;
    let second: i32 = second.parse().ok()?;
    (hour <= 23 && minute <= 59 && second <= 59)
        .then_some(sign * (hour * 3_600 + minute * 60 + second))
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
