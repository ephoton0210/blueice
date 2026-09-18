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
    fn parses_offsets_including_zulu_and_bracket_termination() {
        assert_eq!(parse_offset_seconds("Z"), Some(0));
        assert_eq!(parse_offset_seconds("+01:00"), Some(3_600));
        assert_eq!(parse_offset_seconds("-0130"), Some(-5_400));
        assert_eq!(parse_offset_seconds("+25:00"), None);
    }
}
